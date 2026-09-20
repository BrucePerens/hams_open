# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo import fields
from datetime import timedelta


@tagged("post_install", "-at_install")
class TestScheduleEdgeCases(HamsTransactionCase):
    def test_01_empty_schedule(self):
        """Verify get_current_on_duty_admin returns False when no shifts exist."""
        self.env["calendar.event"].search([], limit=10000).unlink()
        admin = self.env["calendar.event"].get_current_on_duty_admin()
        self.assertFalse(
            admin, "MUST return False when the schedule is completely empty."
        )

    def _make_user(self, suffix):
        return self.env["res.users"].create(
            {"name": "User %s" % suffix, "login": "u_%s" % suffix}
        )

    def _backdate_create_date(self, event, minutes):
        """Push one event's `create_date` `minutes` into the past.

        `create_date` is an Odoo-managed magic column that a plain `write()` will not move, and
        every row created inside one test transaction otherwise shares the identical
        transaction timestamp, so raw SQL is the only way to build a test where the
        `create_date desc` half of the tiebreak is the half that decides. Same pattern as
        `test_incident.py`'s escalation test.
        """
        self.env.cr.execute(
            "UPDATE calendar_event SET create_date = %s WHERE id = %s",
            (fields.Datetime.now() - timedelta(minutes=minutes), event.id),
        )
        event.invalidate_recordset(["create_date"])

    def test_02_overlapping_shifts_same_create_date_highest_id_wins(self):
        """Two shifts created in the same transaction: the higher id is paged.

        Both rows carry the identical `create_date` (PostgreSQL's `now()` is the transaction
        timestamp), so this is exactly the `id desc` half of the tiebreak. Discriminating
        against the previous behavior: `calendar.event._order` is `start desc` with no id
        tiebreak, so with equal `start` values the unordered `search(..., limit=1)` returned an
        arbitrary row -- in practice the LOWEST id, the opposite of what is asserted here.
        """
        # Tests [@ANCHOR: test_pager_notification]
        self.env["calendar.event"].search([], limit=10000).unlink()
        user1 = self._make_user("same_cd_1")
        user2 = self._make_user("same_cd_2")
        now = fields.Datetime.now()

        events = self.env["calendar.event"].create(
            [
                {
                    "name": "Shift 1",
                    "start": now - timedelta(hours=1),
                    "stop": now + timedelta(hours=1),
                    "is_pager_duty": True,
                    "user_id": user1.id,
                },
                {
                    "name": "Shift 2",
                    "start": now - timedelta(hours=1),
                    "stop": now + timedelta(hours=1),
                    "is_pager_duty": True,
                    "user_id": user2.id,
                },
            ]
        )

        self.assertEqual(
            len(set(events.mapped("create_date"))),
            1,
            "Test premise: both shifts MUST share one create_date, or this test is not "
            "exercising the id tiebreak at all.",
        )
        newest = max(events, key=lambda e: e.id)
        self.assertNotEqual(
            events[0].id, events[1].id, "Test premise: the two ids must differ."
        )

        admin = self.env["calendar.event"].get_current_on_duty_admin()
        self.assertEqual(
            admin,
            newest.user_id,
            "With an identical create_date the HIGHEST id MUST win, so the winner is always "
            "one specific admin rather than an implementation-defined one.",
        )

    def test_03_overlapping_shifts_most_recently_created_wins(self):
        """The most recently CREATED overlapping shift is paged, not the newest row id.

        The winner here is deliberately the lower-id shift AND the one whose `start` is
        earlier, so neither `id` order nor `calendar.event`'s own `start desc` default can
        produce this answer by accident: only `create_date desc` can.
        """
        # Tests [@ANCHOR: test_pager_notification]
        self.env["calendar.event"].search([], limit=10000).unlink()
        winner_user = self._make_user("recent_winner")
        loser_user = self._make_user("recent_loser")
        now = fields.Datetime.now()

        winner_shift = self.env["calendar.event"].create(
            {
                "name": "Override shift (created last, starts earlier)",
                "start": now - timedelta(hours=3),
                "stop": now + timedelta(hours=1),
                "is_pager_duty": True,
                "user_id": winner_user.id,
            }
        )
        loser_shift = self.env["calendar.event"].create(
            {
                "name": "Original shift (created first, starts later)",
                "start": now - timedelta(hours=1),
                "stop": now + timedelta(hours=1),
                "is_pager_duty": True,
                "user_id": loser_user.id,
            }
        )
        # Make the LOWER-id row genuinely the most recently created one.
        self._backdate_create_date(loser_shift, minutes=30)

        self.assertLess(
            winner_shift.id,
            loser_shift.id,
            "Test premise: the expected winner must be the lower id, so an id-ordered search "
            "cannot pass this test by coincidence.",
        )
        self.assertGreater(
            winner_shift.create_date,
            loser_shift.create_date,
            "Test premise: the expected winner must be the most recently created shift.",
        )

        admin = self.env["calendar.event"].get_current_on_duty_admin()
        self.assertEqual(
            admin,
            winner_user,
            "The most recently created overlapping shift MUST win, so adding a new shift is "
            "how an admin overrides an existing one.",
        )

    def _assert_global_vs_website_overlap(self, label, newer_is_global):
        """One direction of the global-vs-website overlap: assert the newer of the two wins.

        Factored out of its two callers rather than looped over inside one test, because the
        `search(...).unlink()` reset this needs would then sit inside a loop (a real
        burn-list N+1 finding, not a style preference).
        """
        website = self.env["website"].search([], limit=1)
        self.assertTrue(website, "Test premise: at least one website must exist.")
        now = fields.Datetime.now()
        self.env["calendar.event"].search([], limit=10000).unlink()
        global_user = self._make_user("%s_global" % label)
        site_user = self._make_user("%s_site" % label)

        common = {
            "start": now - timedelta(hours=1),
            "stop": now + timedelta(hours=1),
            "is_pager_duty": True,
        }
        global_shift = self.env["calendar.event"].create(
            dict(common, name="Global shift", website_id=False, user_id=global_user.id)
        )
        site_shift = self.env["calendar.event"].create(
            dict(
                common, name="Website shift", website_id=website.id, user_id=site_user.id
            )
        )
        # Backdate whichever one is meant to be the older of the two.
        if newer_is_global:
            self._backdate_create_date(site_shift, minutes=30)
            expected = global_user
        else:
            self._backdate_create_date(global_shift, minutes=30)
            expected = site_user

        admin = (
            self.env["calendar.event"]
            .with_context(website_id=website.id)
            .get_current_on_duty_admin()
        )
        self.assertEqual(
            admin,
            expected,
            "A global shift and a website shift MUST be ranked purely by which was created "
            "most recently, with no precedence for either.",
        )

    def test_04_newer_global_shift_beats_older_website_shift(self):
        """A global (`website_id=False`) shift wins when it is the more recent one.

        Bruce's rule is "most recent wins" full stop -- "site-specific beats global, then
        latest start wins" was a considered and explicitly rejected alternative -- so a newer
        platform-wide shift really does take the page away from a website's own older rota.
        """
        # Tests [@ANCHOR: test_pager_notification]
        self._assert_global_vs_website_overlap("global_newer", newer_is_global=True)

    def test_05_newer_website_shift_beats_older_global_shift(self):
        """The website's own shift wins when it is the more recent one.

        The other direction of `test_04`, so the pair together show one rule is doing the
        work rather than a hidden global-vs-site preference that happens to agree once.
        """
        # Tests [@ANCHOR: test_pager_notification]
        self._assert_global_vs_website_overlap("site_newer", newer_is_global=False)

    def test_06_ordering_does_not_reach_past_the_time_window(self):
        """The tiebreak only ranks shifts that are actually in force right now.

        A newer, higher-id shift that has not started yet (and an already-expired one) MUST
        NOT displace the older shift that is genuinely on duty -- i.e. `create_date desc`
        orders the domain's matches, it does not widen the domain.
        """
        # Tests [@ANCHOR: test_pager_notification]
        self.env["calendar.event"].search([], limit=10000).unlink()
        on_duty_user = self._make_user("window_current")
        future_user = self._make_user("window_future")
        expired_user = self._make_user("window_expired")
        now = fields.Datetime.now()

        current_shift = self.env["calendar.event"].create(
            {
                "name": "Current shift",
                "start": now - timedelta(hours=1),
                "stop": now + timedelta(hours=1),
                "is_pager_duty": True,
                "user_id": on_duty_user.id,
            }
        )
        # Created after the current shift, so it is the "most recent" by both create_date and
        # id -- but it is out of the window and must be ignored entirely.
        future_shift = self.env["calendar.event"].create(
            {
                "name": "Tomorrow's shift",
                "start": now + timedelta(hours=4),
                "stop": now + timedelta(hours=8),
                "is_pager_duty": True,
                "user_id": future_user.id,
            }
        )
        self.env["calendar.event"].create(
            {
                "name": "Yesterday's shift",
                "start": now - timedelta(hours=8),
                "stop": now - timedelta(hours=4),
                "is_pager_duty": True,
                "user_id": expired_user.id,
            }
        )
        self.assertGreater(
            future_shift.id,
            current_shift.id,
            "Test premise: the out-of-window shift must be the newer row.",
        )

        admin = self.env["calendar.event"].get_current_on_duty_admin()
        self.assertEqual(
            admin,
            on_duty_user,
            "Only shifts in force right now may be ranked; a newer future or expired shift "
            "MUST NOT be paged.",
        )
