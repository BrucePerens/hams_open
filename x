docs/modules/user_websites_seo.md:    * User SEO Write Elevation: `[@ANCHOR: res_users_seo_write_elevation]` (Verified by `test_check_access_rule_res_users`)
docs/modules/user_websites_seo.md:    * Group SEO Write Elevation: `[@ANCHOR: user_websites_group_seo_write_elevation]` (Verified by `test_check_access_rule_user_websites_group`)
docs/modules/user_websites_seo.md:| `[@ANCHOR: res_users_seo_write_elevation]` | Elevated write for user SEO metadata. | `test_check_access_rule_res_users` |
docs/modules/user_websites_seo.md:| `[@ANCHOR: user_websites_group_seo_write_elevation]` | Elevated write for group SEO metadata. | `test_check_access_rule_user_websites_group` |
user_websites/tests/test_exhaustive_isolation.py:        Expected: AccessError from `check_access_rule`.
user_websites_seo/controllers/main.py:            # The models' check_access_rule methods have been enhanced to allow
user_websites_seo/models/res_users.py:            # Verified by [@ANCHOR: test_check_access_rule_res_users]
user_websites_seo/models/user_websites_group.py:            # Verified by [@ANCHOR: test_check_access_rule_user_websites_group]
user_websites_seo/tests/test_seo_models.py:    def test_check_access_rule_res_users(self):
user_websites_seo/tests/test_seo_models.py:        # [@ANCHOR: test_check_access_rule_res_users]
user_websites_seo/tests/test_seo_models.py:        # Verified by [@ANCHOR: test_check_access_rule_res_users]
user_websites_seo/tests/test_seo_models.py:    def test_check_access_rule_user_websites_group(self):
user_websites_seo/tests/test_seo_models.py:        # [@ANCHOR: test_check_access_rule_user_websites_group]
user_websites_seo/tests/test_seo_models.py:        # Verified by [@ANCHOR: test_check_access_rule_user_websites_group]
user_websites_seo/README.md:    * User SEO Write Elevation: `[@ANCHOR: res_users_seo_write_elevation]` (Verified by `test_check_access_rule_res_users`)
user_websites_seo/README.md:    * Group SEO Write Elevation: `[@ANCHOR: user_websites_group_seo_write_elevation]` (Verified by `test_check_access_rule_user_websites_group`)
user_websites_seo/README.md:| `[@ANCHOR: res_users_seo_write_elevation]` | Elevated write for user SEO metadata. | `test_check_access_rule_res_users` |
user_websites_seo/README.md:| `[@ANCHOR: user_websites_group_seo_write_elevation]` | Elevated write for group SEO metadata. | `test_check_access_rule_user_websites_group` |
