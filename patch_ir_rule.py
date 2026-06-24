import traceback
import logging

_logger = logging.getLogger(__name__)

def patch_ir_rule(test_case):
    from odoo.addons.base.models.ir_rule import IrRule
    old_make_access_error = IrRule._make_access_error

    def new_make_access_error(self, operation, records):
        _logger.error("ACCESS ERROR TRIGGERED. TRACEBACK:")
        for line in traceback.format_stack():
            _logger.error(line.strip())
        return old_make_access_error(self, operation, records)

    test_case.patch(IrRule, '_make_access_error', new_make_access_error)
