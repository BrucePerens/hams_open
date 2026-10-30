# SPDX-License-Identifier: AGPL-3.0-or-later
"""The runtime of one bench_mcp procedure run (CPython backend).

bench_mcp starts this file as ``python3 -I -B -u bench_runtime.py
<run_dir>`` in a fresh scratch directory with a stripped
environment. The host speaks newline-delimited JSON with it: requests
go out on a duplicate of the original stdout, replies and cancel
requests come in on stdin. This file installs the audit hook, builds
the ``bench`` module the procedure imports, runs the procedure source
and reports how it ended, with the local variables of the
procedure's own frames when it failed.

THE AUDIT HOOK IS NOT A SANDBOX. It catches accidents and keeps the
tier model honest (a procedure that opens a serial port or starts a
subprocess is told to use ``bench.call``); determined code can get
around it. ``python3 bench_runtime.py --check <source>`` only
compiles a source and lists the tool names it calls literally.
"""

import ast
import builtins
import contextlib
import json
import math
import os
import stat
import sys
import threading
import time
import types
from typing import IO, Any

USER_FILE = "procedure.py"
REPR_LIMIT = 200
LOCALS_PER_FRAME = 30
FRAME_BYTES = 8192
VIOLATION_LIMIT = 50
DEVICE_ALLOWED = (
    "/dev/null",
    "/dev/zero",
    "/dev/urandom",
    "/dev/random",
    "/dev/full",
)
PATH_EVENTS = {
    "os.remove": (0,),
    "os.rename": (0, 1),
    "os.mkdir": (0,),
    "os.rmdir": (0,),
    "os.chmod": (0,),
    "os.chown": (0,),
    "os.truncate": (0,),
    "os.symlink": (0, 1),
    "os.link": (0, 1),
    "os.utime": (0,),
    "shutil.copyfile": (0, 1),
    "shutil.copymode": (0, 1),
    "shutil.copystat": (0, 1),
    "shutil.copytree": (0, 1),
    "shutil.move": (0, 1),
    "shutil.rmtree": (0,),
    "shutil.chown": (0,),
}
BLOCKED = {
    "subprocess.Popen": "start a process",
    "_posixsubprocess.fork_exec": "start a process",
    "os.system": "start a process",
    "os.exec": "start a process",
    "os.posix_spawn": "start a process",
    "os.spawn": "start a process",
    "os.fork": "start a process",
    "os.forkpty": "start a process",
    "pty.spawn": "start a process",
    "os.kill": "signal a process",
    "os.killpg": "signal a process",
    "socket.connect": "open a network connection",
    "socket.bind": "open a network socket",
    "socket.sendto": "send on a network socket",
    "socket.sendmsg": "send on a network socket",
    "socket.getaddrinfo": "look up a network name",
    "socket.gethostbyname": "look up a network name",
    "socket.gethostbyaddr": "look up a network name",
}


def jsonable(value):
    """json.dumps ``default``: numpy scalars and arrays (anything
    with ``tolist``) become plain lists and numbers; other objects
    become their repr."""
    # [@ANCHOR: proc_py_jsonable]
    if hasattr(value, "tolist"):
        return value.tolist()
    if isinstance(value, (set, frozenset, tuple)):
        return list(value)
    if isinstance(value, bytes):
        return value.hex()
    return repr(value)


class BenchError(Exception):
    """A bench tool call failed; ``kind`` says why (tool, tier,
    budget, lock, policy)."""

    def __init__(self, tool, message, kind="tool"):
        # [@ANCHOR: proc_py_bench_error_init]
        super().__init__(f"{tool}: {message}")
        self.tool = tool
        self.message = message
        self.kind = kind


class BenchCancelled(BaseException):
    """The host asked the run to stop (cancel, time-out). Derives
    from BaseException so a procedure's ``except Exception`` does not
    swallow it; registered cleanup handlers still run."""


class ProcedureFailure(Exception):
    """Raised by ``bench.expect``: a failed check with context."""

    def __init__(self, message, context=None):
        # [@ANCHOR: proc_py_failure_init]
        super().__init__(message)
        self.context = context or {}


class PolicyError(RuntimeError):
    """The audit hook refused an operation."""


class Wire:
    """The newline-delimited JSON pipe to the host."""

    def __init__(self, out_fd: int, in_file: IO[str]):
        # [@ANCHOR: proc_py_wire_init]
        self.out_fd = out_fd
        self.in_file = in_file
        self.lock = threading.Lock()
        self.pending: dict[int, dict[str, Any]] = {}
        self.next_id = 0
        self.cancel = threading.Event()
        self.cancel_reason: str | None = None
        self.closed = False

    def send(self, message):
        """Write one message; a closed pipe ends the process."""
        # [@ANCHOR: proc_py_wire_send]
        text = json.dumps(message, default=jsonable, allow_nan=False)
        data = text.encode() + b"\n"
        with self.lock:
            try:
                view = memoryview(data)
                while view:
                    view = view[os.write(self.out_fd, view) :]
            except OSError:
                os._exit(4)

    def request(self, message):
        """Send a request and wait for its reply (or a cancel)."""
        # [@ANCHOR: proc_py_wire_request]
        arrived = threading.Event()
        slot: dict[str, Any] = {"event": arrived, "reply": None}
        with self.lock:
            self.next_id += 1
            ident = self.next_id
            self.pending[ident] = slot
        message["id"] = ident
        self.send(message)
        while not arrived.wait(0.2):
            if self.cancel.is_set() and not STATE["cleanup"]:
                break
        with self.lock:
            self.pending.pop(ident, None)
        if slot["reply"] is None:
            raise BenchCancelled(self.cancel_reason or "cancelled")
        return slot["reply"]

    def reader(self):
        """Thread: route replies to waiting requests, note a cancel,
        and end the process when the host is gone."""
        # [@ANCHOR: proc_py_wire_reader]
        for line in self.in_file:
            try:
                message = json.loads(line)
            except ValueError:
                continue
            op = message.get("op")
            if op == "reply":
                with self.lock:
                    slot = self.pending.get(message.get("id"))
                if slot is not None:
                    slot["reply"] = message
                    slot["event"].set()
            elif op == "cancel":
                self.cancel_reason = message.get(
                    "reason", "cancelled"
                )
                self.cancel.set()
        os._exit(3)


STATE: dict[str, Any] = {
    "cleanup": False,
    "args": {},
    "started": 0.0,
    "wall_s": 0.0,
    "paused": 0.0,
    "cleanups": [],
    "result": None,
    "violations": 0,
    "tools": None,
    "tools_path": None,
    "trace_lines": [],
    "trace_flushed": 0.0,
}
WIRES: list[Wire] = []


def wire() -> Wire:
    """The one Wire of this process (set by main)."""
    # [@ANCHOR: proc_py_wire]
    return WIRES[0]


def check_cancel():
    """Raise BenchCancelled once the host has asked for it."""
    # [@ANCHOR: proc_py_check_cancel]
    if wire().cancel.is_set() and not STATE["cleanup"]:
        raise BenchCancelled(wire().cancel_reason or "cancelled")


def bench_call(tool, **arguments):
    """Call a bench tool through the host's normal dispatch path."""
    # [@ANCHOR: proc_py_call]
    check_cancel()
    message = {"op": "call", "tool": tool, "args": arguments}
    reply = wire().request(message)
    if reply.get("ok"):
        return reply.get("value")
    raise BenchError(
        tool, reply.get("error", "failed"), reply.get("kind", "tool")
    )


def bench_emit(key, value):
    """Store a progress or partial result under ``key``; it is kept
    by the host and survives a kill."""
    # [@ANCHOR: proc_py_emit]
    check_cancel()
    wire().send({"op": "emit", "key": str(key), "value": value})


def bench_log(message):
    """Append a line to the run's event list."""
    # [@ANCHOR: proc_py_log]
    wire().send({"op": "log", "msg": str(message)})


def bench_step(name):
    """Name the step the procedure is in (shown by status)."""
    # [@ANCHOR: proc_py_step]
    check_cancel()
    wire().send({"op": "step", "name": str(name)})


def bench_sleep(seconds):
    """Sleep, ending early with BenchCancelled on a cancel."""
    # [@ANCHOR: proc_py_sleep]
    seconds = float(seconds)
    if not math.isfinite(seconds) or seconds < 0:
        raise ValueError("sleep needs a finite, non-negative time")
    if STATE["cleanup"]:
        time.sleep(seconds)
        return
    check_cancel()
    if wire().cancel.wait(seconds):
        check_cancel()


def bench_checkpoint(name, data):
    """Save ``data`` (JSON) under ``name`` in the run's directory."""
    # [@ANCHOR: proc_py_checkpoint]
    check_cancel()
    wire().send({"op": "checkpoint", "name": str(name), "data": data})


def bench_cleanup(function):
    """Register a function that runs when the procedure ends in
    any way (return, error, cancel, time-out), newest first."""
    # [@ANCHOR: proc_py_cleanup]
    if not callable(function):
        raise TypeError("bench.cleanup needs a function")
    STATE["cleanups"].append(function)
    return function


def bench_tools():
    """The tools this run may call: name -> tier, description,
    schema."""
    # [@ANCHOR: proc_py_tools]
    if STATE["tools"] is None:
        with open(STATE["tools_path"], encoding="utf-8") as handle:
            STATE["tools"] = json.load(handle)
    return STATE["tools"]


def bench_deadline():
    """Seconds of wall-clock budget left (operator waits excluded)."""
    # [@ANCHOR: proc_py_deadline]
    used = time.monotonic() - STATE["started"] - STATE["paused"]
    return STATE["wall_s"] - used


def bench_retry(
    function,
    tries=3,
    delay=0.5,
    backoff=2.0,
    exceptions=(BenchError,),
):
    """Call ``function()`` up to ``tries`` times, sleeping
    ``delay`` seconds (times ``backoff`` after each failure)
    between attempts; the last failure is raised."""
    # [@ANCHOR: proc_py_retry]
    if tries < 1:
        raise ValueError("retry needs at least one try")
    wait = float(delay)
    for attempt in range(1, tries + 1):
        try:
            return function()
        except exceptions as error:
            if attempt == tries:
                raise
            bench_log(f"retry {attempt}/{tries} after: {error}")
            bench_sleep(wait)
            wait *= backoff


def bench_expect(condition, message, **context):
    """Raise a structured ProcedureFailure carrying ``context``
    when the condition is false; returns the condition otherwise."""
    # [@ANCHOR: proc_py_expect]
    if not condition:
        raise ProcedureFailure(message, context)
    return condition


def bench_stats(values):
    """n, mean, sample standard deviation, min, max and peak to
    peak spread of a list of numbers."""
    # [@ANCHOR: proc_py_stats]
    numbers = [float(v) for v in values]
    if not numbers:
        raise ValueError("stats needs at least one value")
    count = len(numbers)
    mean = math.fsum(numbers) / count
    squares = math.fsum((v - mean) ** 2 for v in numbers)
    std = math.sqrt(squares / (count - 1)) if count > 1 else 0.0
    low, high = min(numbers), max(numbers)
    return {
        "n": count,
        "mean": mean,
        "std": std,
        "min": low,
        "max": high,
        "spread": high - low,
    }


def bench_series(
    name,
    count,
    measure,
    retries=2,
    cycle=None,
    cycle_after=2,
    max_failed=None,
):
    """Take ``count`` measurements with ``measure(i)`` (a number).
    A failed measurement (BenchError) is retried ``retries`` times;
    after ``cycle_after`` failures in a row ``cycle()`` is called
    (a power cycle). Points that still fail are listed, and the
    series stops with an error past ``max_failed``. Progress is
    emitted as ``<name>.progress``."""
    # [@ANCHOR: proc_py_series]
    values, failed, cycles, streak = [], [], 0, 0
    limit = count if max_failed is None else max_failed
    for index in range(count):
        error = None
        for _ in range(retries + 1):
            try:
                values.append(float(measure(index)))
                error, streak = None, 0
                break
            except BenchError as caught:
                error = caught
                streak += 1
                if cycle is not None and streak >= cycle_after:
                    bench_log(
                        f"{name}: cycling after {streak} failures"
                    )
                    cycle()
                    cycles += 1
                    streak = 0
        if error is not None:
            failed.append({"index": index, "error": str(error)})
        progress = {"done": index + 1, "of": count}
        progress.update(failed=len(failed), cycles=cycles)
        bench_emit(f"{name}.progress", progress)
        if len(failed) > limit:
            raise ProcedureFailure(
                f"{name}: more than {limit} points failed",
                {"failed": failed, "values": values},
            )
    stats = bench_stats(values) if values else None
    return {
        "name": name,
        "values": values,
        "failed": failed,
        "cycles": cycles,
        "stats": stats,
    }


def bench_reserve(*names, timeout=30):
    """Reserve instruments for this run, waiting up to ``timeout``
    seconds for another run that holds them (all or none)."""
    # [@ANCHOR: proc_py_reserve]
    check_cancel()
    message = {"op": "reserve", "names": list(names)}
    message["timeout"] = timeout
    reply = wire().request(message)
    if not reply.get("ok"):
        raise BenchError(
            "reserve", reply.get("error", "failed"), "lock"
        )


def bench_release(*names):
    """Release instruments reserved with ``bench.reserve``."""
    # [@ANCHOR: proc_py_release]
    reply = wire().request({"op": "release", "names": list(names)})
    if not reply.get("ok"):
        raise BenchError(
            "release", reply.get("error", "failed"), "lock"
        )


@contextlib.contextmanager
def bench_reserved(*names, timeout=30):
    """``with bench.reserved("gen", "scope"):`` reserves for the
    block and releases at its end."""
    # [@ANCHOR: proc_py_reserved]
    bench_reserve(*names, timeout=timeout)
    try:
        yield
    finally:
        bench_release(*names)


def bench_ask_operator(message, timeout=None):
    """Pause the run until the person at the bench has done what
    ``message`` asks. The agent answers with ``bench_proc_input``;
    the answer text is returned. The wait does not count against
    the wall-clock budget but has its own limit (``timeout``, at
    most the runner's operator_timeout_s)."""
    # [@ANCHOR: proc_py_ask_operator]
    check_cancel()
    message = {"op": "ask", "message": str(message)}
    if timeout is not None:
        message["timeout"] = float(timeout)
    begun = time.monotonic()
    try:
        reply = wire().request(message)
    finally:
        STATE["paused"] += time.monotonic() - begun
    if not reply.get("ok"):
        raise BenchError(
            "ask_operator", reply.get("error", "failed"), "policy"
        )
    return reply.get("value")


def bench_result(value):
    """Set the procedure's final value (the last call wins)."""
    # [@ANCHOR: proc_py_result]
    STATE["result"] = value
    return value


def short(value):
    """A bounded repr that never raises."""
    # [@ANCHOR: proc_py_short]
    try:
        text = repr(value)
    except Exception as error:
        text = f"<repr failed: {error.__class__.__name__}>"
    if len(text) > REPR_LIMIT:
        text = text[:REPR_LIMIT] + f"... ({len(text)} characters)"
    return text


def frame_report(frame, line):
    """One frame of the procedure's own code with its locals."""
    # [@ANCHOR: proc_py_frame_report]
    names: dict[str, str] = {}
    used = 0
    for key, value in list(frame.f_locals.items()):
        skip = key.startswith("__") or key == "bench"
        if skip or isinstance(value, types.ModuleType):
            continue
        text = short(value)
        used += len(key) + len(text)
        if len(names) >= LOCALS_PER_FRAME or used > FRAME_BYTES:
            names["..."] = "more variables left out"
            break
        names[key] = text
    return {
        "function": frame.f_code.co_name,
        "line": line,
        "locals": names,
    }


def failure_report(error):
    """The structured report of an uncaught exception: type,
    message, traceback text, the procedure's own frames with their
    local variables, and the context of bench.expect."""
    # [@ANCHOR: proc_py_failure_report]
    import traceback

    frames = []
    trace = error.__traceback__
    while trace is not None:
        code = trace.tb_frame.f_code
        if code.co_filename == USER_FILE:
            frames.append(
                frame_report(trace.tb_frame, trace.tb_lineno)
            )
        trace = trace.tb_next
    lines = traceback.format_exception(error)
    report = {
        "type": error.__class__.__name__,
        "message": str(error),
        "traceback": "".join(lines)[-8000:],
        "frames": frames,
    }
    if isinstance(error, ProcedureFailure):
        text = json.dumps(error.context, default=jsonable)
        report["context"] = json.loads(text.replace("NaN", "null"))
    if isinstance(error, BenchError):
        report["tool"] = error.tool
        report["kind"] = error.kind
    if isinstance(error, SyntaxError):
        report["line"] = error.lineno
    return report


def path_inside(path, scratch):
    """True when ``path`` resolves to the scratch directory or
    below it."""
    # [@ANCHOR: proc_py_path_inside]
    try:
        real = os.path.realpath(os.fsdecode(path))
    except (TypeError, ValueError):
        return False
    return real == scratch or real.startswith(scratch + os.sep)


def open_refusal(args, scratch):
    """Why an ``open`` event is refused, or None. Device nodes are
    refused for reading too; writing is allowed only inside the
    scratch directory (and /dev/null)."""
    # [@ANCHOR: proc_py_open_refusal]
    target, mode, flags = args[0], args[1], args[2]
    if not isinstance(target, (str, bytes, os.PathLike)):
        return None
    real = os.path.realpath(os.fsdecode(target))
    try:
        kind = os.stat(real).st_mode
    except OSError:
        kind = 0
    device = stat.S_ISCHR(kind) or stat.S_ISBLK(kind)
    if device and real not in DEVICE_ALLOWED:
        return "open the device node " + real
    writing = isinstance(flags, int) and (
        flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_APPEND)
    )
    if isinstance(mode, str) and set(mode) & set("wax+"):
        writing = True
    if (
        writing
        and real != "/dev/null"
        and not path_inside(real, scratch)
    ):
        return "write " + real + " (outside the scratch directory)"
    return None


def make_hook(scratch):
    """The audit hook function for this run."""
    # [@ANCHOR: proc_py_make_hook]
    busy = threading.local()

    def refusal(event, args):
        if event in BLOCKED:
            return BLOCKED[event]
        if event.startswith("ctypes."):
            return "use the ctypes module"
        if event == "open":
            return open_refusal(args, scratch)
        for index in PATH_EVENTS.get(event, ()):
            inside = path_inside(args[index], scratch)
            if not inside:
                return f"{event} {args[index]!r} outside scratch"
        return None

    def hook(event, args):
        if getattr(busy, "on", False):
            return
        busy.on = True
        try:
            why = refusal(event, args)
            if why is None:
                return
            text = (
                f"blocked: this procedure tried to {why} "
                f"({event}). Use bench.call(tool, ...) for "
                "instruments; the files you may write are in "
                "the scratch directory."
            )
            STATE["violations"] += 1
            if STATE["violations"] <= VIOLATION_LIMIT:
                wire().send(
                    {"op": "violation", "event": event, "msg": text}
                )
            raise PolicyError(text)
        finally:
            busy.on = False

    return hook


class Output:
    """A file-like object that sends text to the host, which stores
    it with a size cap."""

    def __init__(self, name):
        # [@ANCHOR: proc_py_output_init]
        self.name = name

    def write(self, text):
        # [@ANCHOR: proc_py_output_write]
        if text:
            wire().send({"op": self.name, "text": str(text)})
        return len(text)

    def flush(self):
        # [@ANCHOR: proc_py_output_flush]
        return None

    def isatty(self):
        # [@ANCHOR: proc_py_output_isatty]
        return False


def make_bench(arguments):
    """Build the ``bench`` module the procedure sees."""
    # [@ANCHOR: proc_py_make_bench]
    module = types.ModuleType("bench")
    module.__doc__ = "bench_mcp procedure API"
    table = {
        "call": bench_call,
        "emit": bench_emit,
        "log": bench_log,
        "step": bench_step,
        "sleep": bench_sleep,
        "checkpoint": bench_checkpoint,
        "cleanup": bench_cleanup,
        "tools": bench_tools,
        "deadline": bench_deadline,
        "retry": bench_retry,
        "expect": bench_expect,
        "stats": bench_stats,
        "series": bench_series,
        "ask_operator": bench_ask_operator,
        "reserve": bench_reserve,
        "release": bench_release,
        "reserved": bench_reserved,
        "result": bench_result,
        "args": arguments,
        "BenchError": BenchError,
        "BenchCancelled": BenchCancelled,
        "ProcedureFailure": ProcedureFailure,
        "PolicyError": PolicyError,
    }
    for key, value in table.items():
        setattr(module, key, value)
    return module


def tracer(frame, event, argument):
    """sys.settrace function: note the line numbers the
    procedure's own code reaches (batched)."""
    # [@ANCHOR: proc_py_tracer]
    if frame.f_code.co_filename != USER_FILE:
        return None
    if event == "line":
        STATE["trace_lines"].append(frame.f_lineno)
        now = time.monotonic()
        full = len(STATE["trace_lines"]) >= 50
        if full or now - STATE["trace_flushed"] > 0.5:
            flush_trace()
            STATE["trace_flushed"] = now
    return tracer


def flush_trace():
    """Send the traced line numbers collected so far."""
    # [@ANCHOR: proc_py_flush_trace]
    lines = STATE["trace_lines"]
    if lines:
        STATE["trace_lines"] = []
        wire().send({"op": "trace", "lines": lines})


def run_cleanups():
    """Run the registered cleanup handlers, newest first. A handler
    that fails is reported and does not stop the others."""
    # [@ANCHOR: proc_py_run_cleanups]
    STATE["cleanup"] = True
    problems = []
    while STATE["cleanups"]:
        function = STATE["cleanups"].pop()
        try:
            function()
        except BaseException as error:
            text = f"cleanup {short(function)} failed: {error!r}"
            problems.append(text)
            bench_log(text)
    return problems


def execute(code, namespace):
    """Run the compiled procedure and say how it ended: a
    ``(outcome, payload)`` pair."""
    # [@ANCHOR: proc_py_execute]
    try:
        exec(code, namespace)
        value = STATE["result"]
        if value is None:
            value = namespace.get("result")
        text = json.dumps(value, default=jsonable, allow_nan=False)
        return "done", {"value": json.loads(text)}
    except BenchCancelled as error:
        return "cancelled", {
            "reason": str(error),
            **failure_report(error),
        }
    except SystemExit as error:
        if error.code in (None, 0):
            return "done", {"value": STATE["result"]}
        return "failed", failure_report(error)
    except BaseException as error:
        return "failed", failure_report(error)


def check_source(path):
    """``--check``: compile the source and list the tool names it
    passes to ``bench.call`` as string literals."""
    # [@ANCHOR: proc_py_check_source]
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    try:
        tree = ast.parse(text, USER_FILE)
        compile(tree, USER_FILE, "exec")
    except SyntaxError as error:
        report = {
            "ok": False,
            "error": str(error),
            "line": error.lineno,
        }
        return report
    names = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        func = node.func
        named = (
            isinstance(func, ast.Attribute) and func.attr == "call"
        )
        if (
            named
            and node.args
            and isinstance(node.args[0], ast.Constant)
        ):
            if isinstance(node.args[0].value, str):
                names.add(node.args[0].value)
    return {"ok": True, "calls": sorted(names)}


def main(argv):
    """Entry point: ``--check <source>`` or ``<run_dir>``."""
    # [@ANCHOR: proc_py_main]
    if len(argv) == 3 and argv[1] == "--check":
        print(json.dumps(check_source(argv[2])))
        return 0
    run_dir = argv[1]
    with open(
        os.path.join(run_dir, "config.json"), encoding="utf-8"
    ) as fh:
        config = json.load(fh)
    out_fd = os.dup(1)
    devnull = os.open(os.devnull, os.O_WRONLY)
    os.dup2(devnull, 1)
    os.close(devnull)
    scratch = os.path.realpath(config["scratch"])
    WIRES.append(
        Wire(out_fd, open(0, encoding="utf-8", closefd=False))
    )
    sys.stdin = open(os.devnull, encoding="utf-8")
    sys.stdout = Output("stdout")
    sys.stderr = Output("stderr")
    STATE["args"] = config["args"]
    STATE["wall_s"] = float(config["wall_s"])
    STATE["tools_path"] = os.path.join(run_dir, "tools.json")
    with open(
        os.path.join(run_dir, "source.py"), encoding="utf-8"
    ) as fh:
        source = fh.read()
    threading.Thread(target=wire().reader, daemon=True).start()
    bench = make_bench(config["args"])
    sys.modules["bench"] = bench
    namespace = {"__name__": "__main__", "bench": bench}
    namespace["__builtins__"] = builtins
    STATE["started"] = time.monotonic()
    wire().send({"op": "ready", "python": sys.version.split()[0]})
    try:
        code = compile(source, USER_FILE, "exec")
    except SyntaxError as error:
        report = failure_report(error)
        wire().send({"op": "failed", "report": report})
        return 0
    import ctypes  # noqa: F401 (numpy imports it; loaded before the hook)
    import linecache

    lines = source.splitlines(True)
    linecache.cache[USER_FILE] = (len(source), None, lines, USER_FILE)
    sys.addaudithook(make_hook(scratch))
    if config.get("trace"):
        sys.settrace(tracer)
    outcome, payload = execute(code, namespace)
    sys.settrace(None)
    flush_trace()
    problems = run_cleanups()
    payload["cleanup_problems"] = problems
    wire().send({"op": outcome, "report": payload})
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
