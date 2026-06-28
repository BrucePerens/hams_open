# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import odoo.tests
from odoo.tests import tagged
import secrets
import os
import logging
import time

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install", "-standard", "simulation")
class TestLongRunningSimulation(odoo.tests.common.HttpCase):
    """
    Executes a high-speed simulation to exercise system capabilities
    over a specified number of iterations.
    """

    def setUp(self):
        super(TestLongRunningSimulation, self).setUp()
        self.admin = self.env.ref("base.user_admin")
        self.article = None

        # Ensure the admin has the explicit User Websites Administrator group for testing access rules
        self.admin.write(
            {
                "group_ids": [
                    (
                        4,
                        self.env.ref(
                            "user_websites.group_user_websites_administrator"
                        ).id,
                    )
                ]
            }
        )

        # Setup an active community of 20 users
        self.users = []
        for i in range(20):
            u = self.env["res.users"].create(
                {
                    "name": f"Sim User {i}",
                    "login": f"simuser{i}",
                    "email": f"sim{i}@example.com",
                    "website_slug": f"simuser{i}",
                    "privacy_show_in_directory": True,
                    "group_ids": [
                        (
                            6,
                            0,
                            [
                                self.env.ref("base.group_portal").id,
                                self.env.ref(
                                    "user_websites.group_user_websites_user"
                                ).id,
                            ],
                        )
                    ],
                    "password": "password",
                }
            )
            self.users.append(u)

        # Setup manual library baseline unconditionally since it is a hard dependency
        self.article = self.env["knowledge.article"].create(
            {
                "name": "Simulation Survival Guide",
                "body": "<p>This is a guide for the simulated environment.</p>",
                "is_published": True,
            }
        )

    def _execute_simulation_step(self, i, iterations, metrics):
        """Helper method to isolate ORM operations from the AST loop depth counter."""
        _logger.info(f"[*] === Starting Simulation Step {i + 1} / {iterations} ===")
        user = secrets.choice(self.users)

        def track(op_name, func, *args, **kwargs):
            start = time.time()
            res = func(*args, **kwargs)
            duration = time.time() - start
            if op_name not in metrics:
                metrics[op_name] = []
            metrics[op_name].append(duration)
            return res

        # 1. Unauthenticated / Guest Actions
        self.authenticate(None, None)
        track("Guest: Community Directory", self.url_open, "/community")
        track("Guest: Knowledge", self.url_open, "/manual")
        track("Guest: Privacy Policy", self.url_open, "/privacy")
        track("Guest: Terms of Service", self.url_open, "/terms")

        # 2. Authenticated Content Creation & Interaction
        track("Auth: Login", self.authenticate, user.login, "password")

        # Lazily provision the personal site and blog (safe to repeat)
        track(
            "User: Create Site",
            self.url_open,
            f"/{user.website_slug}/create_site",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )
        track(
            "User: Create Blog",
            self.url_open,
            f"/{user.website_slug}/create_blog",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )

        # Interact with the manual library safely without getattr
        if self.article:
            track(
                "User: Manual Feedback",
                self.url_open,
                "/manual/feedback",
                data={
                    "csrf_token": odoo.http.Request.csrf_token(self),
                    "article_id": self.article.id,
                    "is_helpful": secrets.choice(["0", "1"]),
                },
                method="POST",
            )

        # GDPR Portability check
        track(
            "User: GDPR Export",
            self.url_open,
            "/my/privacy/export",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )

        # 3. Community Moderation (Abuse Reporting)
        # Randomly report a violation against another user
        other_user = secrets.choice([u for u in self.users if u.id != user.id])
        track(
            "User: Report Violation",
            self.url_open,
            "/website/report_violation",  # burn-ignore-route
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "url": f"/{other_user.website_slug}/home",
                "description": f"Simulated violation report in iteration {i}.",
                "email": user.email,
            },
            method="POST",
        )

        # 4. Administrative Processing
        track("Admin: Login", self.authenticate, "admin", "admin")

        # Admin processes the queue
        def admin_process():
            reports = (
                self.env["content.violation.report"]
                .with_user(self.admin)
                .search([("state", "=", "new")], limit=10)
            )
            for report in reports:
                action = secrets.choice(["dismiss", "strike"])
                if action == "dismiss":
                    report.action_dismiss()
                else:
                    report.action_take_action_and_strike()

        track("Admin: Process Reports", admin_process)

        # 5. Appeal & Pardon Lifecycle
        if other_user.is_suspended_from_websites:
            # Suspended user submits an appeal
            track(
                "User: Login (Suspended)",
                self.authenticate,
                other_user.login,
                "password",
            )
            track(
                "User: Submit Appeal",
                self.url_open,
                "/website/submit_appeal",  # burn-ignore-route
                data={
                    "csrf_token": odoo.http.Request.csrf_token(self),
                    "reason": "I am a simulation. Please pardon my simulated behavior.",
                },
                method="POST",
            )

            # Admin reviews and pardons
            track("Admin: Login (Pardon)", self.authenticate, "admin", "admin")

            def admin_pardon():
                appeal = (
                    self.env["content.violation.appeal"]
                    .with_user(self.admin)
                    .search(
                        [("user_id", "=", other_user.id), ("state", "=", "new")],
                        limit=1,
                    )
                )
                if appeal:
                    appeal.action_approve()

            track("Admin: Approve Appeal", admin_pardon)

        # Finalize loop state and flush to DB before next iteration
        track("System: Flush DB", self.env.flush_all)

        if i < iterations - 1:
            _logger.info(
                f"[*] === Step {i + 1} Complete. Proceeding to next step... ==="
            )

    def test_01_high_speed_full_platform_exercise(self):
        # [@ANCHOR: simulation_environment]
        # Tests [@ANCHOR: simulation_environment]
        # Use the variable as an iteration count instead of minutes now
        iterations = int(os.environ.get("SIMULATION_DURATION_MINUTES") or "60")

        # Flush the setup state so DB reflects latest ORM creations
        self.env.flush_all()
        metrics = {}

        for i in range(iterations):
            self._execute_simulation_step(i, iterations, metrics)

        # Performance Evaluation at the end
        _logger.info("==========================================================")
        _logger.info(" 🚀 SIMULATION PERFORMANCE METRICS")
        _logger.info("==========================================================")

        regression_detected = False
        fallback_threshold = float(os.environ.get("SIMULATION_MAX_AVG_TIME") or "0.5")

        # Override defaults for naturally heavier transactions
        custom_thresholds = {
            "User: GDPR Export": 1.0,
            "Admin: Process Reports": 1.5,
            "System: Flush DB": 1.5,
        }

        for op, times in metrics.items():
            if not times:
                continue
            avg_time = sum(times) / len(times)
            max_time = max(times)
            min_time = min(times)
            thresh = custom_thresholds.get(op, fallback_threshold)

            msg = f"{op:<30} | Avg: {avg_time:.4f}s | Max: {max_time:.4f}s | Min: {min_time:.4f}s"

            if avg_time > thresh:
                _logger.warning(f"[REGRESSION] {msg} (Threshold: {thresh}s)")
                regression_detected = True
            else:
                _logger.info(f"[OK]         {msg}")

        if regression_detected:
            _logger.warning(
                "=========================================================="
            )
            _logger.warning(" ⚠️ SPEED REGRESSIONS DETECTED DURING SIMULATION")
            _logger.warning(
                " Check the [REGRESSION] tags above to identify the slow operations."
            )
            _logger.warning(
                " Note: This is a performance warning, not a hard test failure."
            )
            _logger.warning(
                "=========================================================="
            )
        else:
            _logger.info("==========================================================")
            _logger.info(" ✅ ALL OPERATIONS PERFORMED WITHIN ACCEPTABLE LIMITS")
            _logger.info("==========================================================")
