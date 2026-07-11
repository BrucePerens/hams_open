# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from . import caching_mixin

# CRITICAL LOAD ORDER: 'website' MUST be imported BEFORE 'res_config_settings'.
# The related fields in res_config_settings depend on the fields being fully
# materialized on the website model first.
from . import website
from . import res_config_settings
