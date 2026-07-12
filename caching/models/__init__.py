# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.

# CRITICAL LOAD ORDER: 'website' MUST be imported BEFORE 'res_config_settings'.
# The related fields in res_config_settings depend on the fields being fully
# materialized on the website model first.
