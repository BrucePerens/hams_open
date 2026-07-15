# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from .hooks import post_init_hook
from . import models
from . import controllers
from . import wizard
import logging

_logger = logging.getLogger(__name__)
