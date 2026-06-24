import odoo
from odoo.tests.common import TransactionCase

class TestCheckUid(TransactionCase):
    def test_check_uid(self):
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("ham_base.user_manager_service")
        public_uid = self.env.ref("base.public_user").id
        print(f"================================\nSVC UID IS {svc_uid}\nPUBLIC UID IS {public_uid}\n================================")
