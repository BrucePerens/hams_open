# -*- coding: utf-8 -*-
import importlib.util
import logging

_logger = logging.getLogger(__name__)

REQUIRED_DEPS = ["redis", "asyncpg"]
missing = [dep for dep in REQUIRED_DEPS if importlib.util.find_spec(dep) is None]

if missing:
    apt_pkgs = " ".join([f"python3-{m}" for m in missing])
    msg = (
        "\n================================================================================\n"
        "🚨 CRITICAL DEPENDENCY ERROR 🚨\n"
        f"The 'distributed_redis_cache' module is missing required Python modules: {', '.join(missing)}\n\n"
        f"Debian/Ubuntu users run: sudo apt-get install {apt_pkgs}\n"
        f"Pip users run: pip3 install {' '.join(missing)}\n"
        "================================================================================\n"
    )
    _logger.error(msg)
    raise ImportError(msg)

from . import models  # noqa: E402
from .hooks import post_init_hook  # noqa: E402
