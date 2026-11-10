# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for hams_test_depends.

Tests [@ANCHOR: hams_test_depends_check_entries]
Tests [@ANCHOR: hams_test_depends_plan]
Tests [@ANCHOR: hams_test_depends_reaches]
Tests [@ANCHOR: hams_test_depends_queue_companions]
Tests [@ANCHOR: hams_test_depends_install_patches]
"""

from odoo.exceptions import UserError
from odoo.tests import BaseCase, tagged

from odoo.addons.hams_test_depends import companions

GRAPH = {
    'a': {'depends': ['base'], 'test_depends': ['b']},
    'b': {'depends': ['base']},
    'c': {'depends': ['b']},
    'd': {'depends': ['base'], 'test_depends': ['d']},
    'e': {'depends': ['base'], 'test_depends': ['b']},
    'f': {'depends': ['b'], 'test_depends': ['a']},
    'g': {'depends': ['base'], 'test_depends': ['nope']},
    'h': {'depends': ['a'], 'test_depends': ['i']},
    'i': {'depends': ['base'], 'test_depends': ['h']},
    'j': {'depends': ['base'], 'test_depends': ['a']},
}


def manifest_of(name):
    return GRAPH.get(name, {'depends': ['base']})


def exists(name):
    return name in GRAPH or name == 'base'


class FakeRecords(list):
    def mapped(self, field):
        return list(self)

    def search(self, domain):
        return self


class FakeEnv(dict):
    """An env whose ir.module.module holds `names` (all uninstalled)."""

    def __init__(self, names):
        super().__init__()
        self.names = names

    def __getitem__(self, model):
        return FakeModule(self.names)


class FakeModule:
    def __init__(self, names):
        self.names = names

    def search(self, domain):
        wanted = [c for c in domain if c[0] == 'name' and c[1] == 'in']
        names = [n for n in self.names if not wanted or n in wanted[0][2]]
        return FakeRecords(names)


@tagged('post_install', '-at_install')
class TestCompanions(BaseCase):

    def test_good_entry_is_planned(self):
        self.assertEqual(companions.plan(['a'], manifest_of, exists), ['b'])

    def test_companions_of_companions_are_planned(self):
        self.assertEqual(companions.plan(['j'], manifest_of, exists), ['a', 'b'])

    def test_targets_are_never_companions_of_themselves(self):
        self.assertEqual(companions.plan(['a', 'b'], manifest_of, exists), [])

    def test_no_key_means_nothing(self):
        self.assertEqual(companions.plan(['b', 'c'], manifest_of, exists), [])

    def test_bad_entries_are_refused_with_a_clear_message(self):
        cases = {
            'd': 'lists the module itself',
            'c': None,
            'g': 'not an available module',
            'h': 'cycle',
        }
        for module, message in cases.items():
            if message is None:
                continue
            with self.subTest(module=module), self.assertRaises(UserError) as caught:
                companions.plan([module], manifest_of, exists)
            self.assertIn(message, str(caught.exception))

    def test_redundant_entry_is_refused(self):
        graph = dict(GRAPH, e={'depends': ['b'], 'test_depends': ['b']})
        with self.assertRaises(UserError) as caught:
            companions.plan(['e'], lambda n: graph.get(n, {'depends': ['base']}), exists)
        self.assertIn('already in its depends', str(caught.exception))

    def test_a_companion_depending_on_the_module_is_allowed(self):
        graph = dict(GRAPH, a={'depends': ['base'], 'test_depends': ['f']})  # f depends on b, not on a here
        graph['f'] = {'depends': ['a']}
        self.assertEqual(companions.plan(['a'], lambda n: graph.get(n, {'depends': ['base']}), exists), ['f'])

    def test_queue_companions_marks_an_uninstalled_module_to_install(self):
        env, marked = FakeEnv(['zeta']), []
        fake = {'alpha': {'depends': ['base'], 'test_depends': ['zeta']}, 'zeta': {'depends': ['base']}}
        names = companions.queue_companions(env, targets=['alpha'], manifest_of=lambda n: fake.get(n, {'depends': ['base']}),
                                            button_install=lambda mods: marked.extend(mods.mapped('name')))
        self.assertEqual((names, marked), (['zeta'], ['zeta']))

    def test_queue_companions_with_an_unknown_entry_raises(self):
        with self.assertRaises(UserError):
            companions.queue_companions(FakeEnv(['zeta']), targets=['base'],
                                        manifest_of=lambda n: {'depends': [], 'test_depends': ['no_such_mod']},
                                        button_install=lambda mods: None)

    def test_patches_are_installed_once(self):
        companions.install_patches()
        self.assertIn('button_install', companions._ORIGINAL)
        before = dict(companions._ORIGINAL)
        companions.install_patches()
        self.assertEqual(companions._ORIGINAL, before)

