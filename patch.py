import traceback
from odoo.addons.base.models.res_users import Groups
old_write = Groups.write
def new_write(self, vals):
    if self.env.uid == 3:
        print("====== GROUPS WRITE WITH UID 3 ======")
        traceback.print_stack()
        print("VALS:", vals)
    return old_write(self, vals)
Groups.write = new_write
