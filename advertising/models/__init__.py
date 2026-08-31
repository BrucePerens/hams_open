# Copyright © HAMS project. AGPL-3.0-or-later.
# CRITICAL LOAD ORDER: 'website' MUST be imported before 'res_config_settings' --
# res_config_settings' related fields need website's fields materialized first,
# matching caching/models/__init__.py's own established convention.
from . import website
from . import res_config_settings
