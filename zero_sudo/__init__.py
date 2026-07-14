# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import logging
from . import models
from . import controllers


class TEscWarningFilter(logging.Filter):
    """
    Suppresses noisy Odoo 15+ QWeb deprecation warnings for @t-esc
    to prevent polluting test logs.
    """

    def filter(self, record):
        msg = record.getMessage()
        if "@t-esc" in msg and "deprecated" in msg.lower():
            return False
        return True


# Attach to the root logger to intercept messages from ir.qweb or any view rendering
logging.getLogger().addFilter(TEscWarningFilter())

# [@ANCHOR: zero_sudo:COMM_zero_sudo_doc_installer]
