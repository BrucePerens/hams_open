# test_hasattr.py
def do_something(obj):
    if hasattr(obj, 'foo'):
        pass
    
    try:
        val = getattr(obj, 'bar', 'default')
    except AttributeError:
        pass
