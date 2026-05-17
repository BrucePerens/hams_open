env['knowledge.article'].create({
    'name': 'Verification Article',
    'body': '<h2>Overview</h2><p>This is a verification article.</p><h3>Details</h3><p>More details here.</p>',
    'is_published': True,
})
env.cr.commit()
