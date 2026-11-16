# [@ANCHOR: test_persona_login]
# Tests [@ANCHOR: persona_login]
def test_persona_login():
    from zero_sudo.models.model import persona_login
    assert persona_login() == 1
