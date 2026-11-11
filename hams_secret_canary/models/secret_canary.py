# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import json
import logging
import os
import time

from odoo import api, models
from odoo.tools import config

from .. import alerts, detector

_logger = logging.getLogger(__name__)
STATE_KEY = "hams_secret_canary.state"
CACHE_SECONDS = 30
# dbname -> (loaded_at, salt, {digest: (label, kind)}); the response scan refreshes it every CACHE_SECONDS, the log
# watcher reads whatever is loaded.
_tables = {}
_original_factory = []


# [@ANCHOR: hams_secret_canary:watch_log_records]
def _watch_log_records(factory):
    def make_record(*args, **kwargs):
        record = factory(*args, **kwargs)
        if _tables and not record.name.startswith("hams_secret_canary"):
            message = record.getMessage()
            for _loaded, salt, table in list(_tables.values()):
                matches = detector.find_matches(message, salt, table)
                if matches:
                    record.msg, record.args = detector.redact(message, salt, table), None
                    for label, kind in matches:
                        alerts.logging_backend(None, alerts.Alarm(label, kind, "log", record.name))
                    break
        return record
    return make_record


class SecretCanary(models.AbstractModel):
    _name = "hams.secret.canary"
    _description = "Secret Canary"

    @api.model
    # [@ANCHOR: hams_secret_canary:register_hook]
    def _register_hook(self):
        super()._register_hook()
        self._load_state()
        self._plant_decoy_file_and_env()
        if not _original_factory:
            _original_factory.append(logging.getLogRecordFactory())
            logging.setLogRecordFactory(_watch_log_records(_original_factory[0]))

    @api.model
    # [@ANCHOR: hams_secret_canary:load_state]
    def _load_state(self):
        """Returns `(salt, table)` for this database, read from the key-value store at most every `CACHE_SECONDS`
        seconds per process. `(None, {})` before the module's post-init hook has run."""
        dbname = self.env.cr.dbname
        cached = _tables.get(dbname)
        if cached and time.monotonic() - cached[0] < CACHE_SECONDS:
            return cached[1], cached[2]
        raw = self.env["zero_sudo.security.utils"]._get_kv(STATE_KEY)
        state = json.loads(raw) if raw else {"salt": None, "table": {}}
        table = {digest: tuple(entry) for digest, entry in state["table"].items()}
        _tables[dbname] = (time.monotonic(), state["salt"], table)
        return state["salt"], table

    @api.model
    # [@ANCHOR: hams_secret_canary:store_state]
    def _store_state(self, salt, table):
        self.env["zero_sudo.security.utils"]._set_kv(STATE_KEY, json.dumps({"salt": salt, "table": table}))
        _tables.pop(self.env.cr.dbname, None)

    @api.model
    # [@ANCHOR: hams_secret_canary:register_secret]
    def _register_secret(self, label, value, kind=detector.KIND_SECRET):
        """Registers the fingerprint of `value` under `label` so that its appearance in a response or log line raises
        an alarm. Only the salted hash is stored. `value` must be exactly one token (`detector.is_token`); raises
        `ValueError` otherwise. Private: not callable over RPC."""
        if not detector.is_token(value):
            raise ValueError("%s is not a single 16-512 character token of letters, digits and _-+/." % label)
        salt, table = self._load_state()
        if salt is None:
            salt = detector.new_salt()
        table = dict(table)
        table[detector.fingerprint(salt, value)] = (label, kind)
        self._store_state(salt, table)

    @api.model
    # [@ANCHOR: hams_secret_canary:plant_canaries]
    def _plant_canaries(self):
        """Creates the salt, then for each shape in `detector.CANARY_SHAPES` not yet planted generates a decoy value,
        registers its fingerprint as a canary and stores the value as the system parameter `<label>_old`
        (`stripe_secret_key_old` and so on): where an attacker reads first. An existing parameter of that name is left
        alone and not registered."""
        salt, table = self._load_state()
        registered = {label for label, kind in table.values() if kind == detector.KIND_CANARY}
        params = self.env["zero_sudo.security.utils"]._get_service_env("zero_sudo.config_service_internal")["ir.config_parameter"]
        for label in detector.CANARY_SHAPES:
            key = "%s_old" % label
            if label in registered or params.get_param(key):
                continue
            value = detector.make_canary(label)
            self._register_secret(label, value, detector.KIND_CANARY)
            params.set_param(key, value)
        self._plant_decoy_file_and_env()

    @api.model
    # [@ANCHOR: hams_secret_canary:plant_decoy_file_and_env]
    def _plant_decoy_file_and_env(self):
        """Puts each planted decoy where code that reads the environment or the key directory looks: an environment
        variable `<LABEL>_OLD` in this process (not overriding one that is set) and a file `<LABEL>_old` (mode 0600)
        in the `keys` directory of the Odoo data directory (not replacing one that exists). The values are the canaries
        already registered, so a leak of either is noticed by the same scan. Best effort: a directory or file that
        cannot be written is logged and skipped."""
        params = self.env["zero_sudo.security.utils"]._get_service_env("zero_sudo.config_service_internal")["ir.config_parameter"]
        directory = os.path.join(config["data_dir"], "keys")
        for label in detector.CANARY_SHAPES:
            value = params.get_param("%s_old" % label)
            if not value:
                continue
            os.environ.setdefault("%s_OLD" % label.upper(), value)
            path = os.path.join(directory, "%s_old" % label)
            if os.path.exists(path):
                continue
            try:
                os.makedirs(directory, mode=0o700, exist_ok=True)
                descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)  # audit-ignore-path: fixed label under the data directory
                with os.fdopen(descriptor, "w") as handle:
                    handle.write(value + "\n")
            except OSError as error:
                _logger.warning("Decoy key file %s not written: %s", label, type(error).__name__)

    @api.model
    # [@ANCHOR: hams_secret_canary:scan_text]
    def _scan_text(self, text, source, where):
        """Raises an alarm (`alerts.raise_alarm`) for every canary or registered secret in `text`; returns the list of
        `(label, kind)` found. `source` is "response" or "log"; `where` is the request path, redacted before use."""
        salt, table = self._load_state()
        if not table:
            return []
        matches = detector.find_matches(text, salt, table)
        if matches:
            where = detector.redact(where, salt, table)
            for label, kind in matches:
                alerts.raise_alarm(self.env, alerts.Alarm(label, kind, source, where))
        return matches
