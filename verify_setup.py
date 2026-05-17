# Retrieve the injected env object from the shell runtime to satisfy AST linters natively
odoo_env = globals().get("env") or locals().get("env")

odoo_env['knowledge.article'].create({
    'name': 'Verification Article',
    'body': '<h2>Overview</h2><p>This is a verification article.</p><h3>Details</h3><p>More details here.</p>',
    'is_published': True,
})
odoo_env.cr.commit()
