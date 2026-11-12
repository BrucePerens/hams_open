<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Declaring a method meant for RPC `[@ANCHOR: zero_sudo:COMM_story_rpc_public]`

A public method of a model is callable by any signed-in user over `/web/dataset/call_kw`. Most of our public methods are only called from Python and should be private (leading underscore, or `@api.private`). The few that are really meant to be called over RPC (a `type="object"` view button, a JS `orm.call`, a daemon, an override of a core method) are declared with `@public_rpc` `[@ANCHOR: zero_sudo:public_rpc]`:

```python
from odoo.addons.zero_sudo import public_rpc

class Thing(models.Model):
    _inherit = "thing"

    @public_rpc
    def action_confirm(self):
        ...
```

The decorator sets one attribute and changes nothing else. It lives in `zero_sudo` so an addon does not need a dependency on `hams_rpc_guard`. When `hams_rpc_guard` is installed it reads the attribute and admits the call (`@public_rpc(portal=True)` also admits portal and public users, `@public_rpc(clients=("login",))` admits those service accounts); its source scanner skips a decorated method, so the addon's `rpc_exposure.json` does not list it.
