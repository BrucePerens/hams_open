# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Raw inbound messages shaped like the production tickets of 2026-10-28 to 2026-11-03, shared by the helpdesk and
pager_duty tests. Only the shape of the real mail (HTML marketing newsletter: nested layout tables, images, a
tracking pixel, "view in browser" and "unsubscribe" footers, bulk-mailer headers, no plain-text alternative) and the
real ticket names are reproduced; the text is invented and none of it comes from a real message."""
import hashlib

# Ticket names of hams_prod that were created at stage "new" although they are not real requests (ids 113-140).
REAL_MISSED_SPAM_NAMES = (
    "Beyond standard inhalers: A natural nutritional approach for lungs",
    "The natural evening compound addressing neural energy decline",
    "Watch before deleted: The simple ritual resetting BP to 118/78",
    "The secret behind Wellamoon sleep support",
    "Why researchers are looking at this simple kitchen baking soda",
    "What to Know About Tingling Feet",
)

# Ticket names of hams_prod that are not mail from a stranger: server exceptions, monitoring from our own systems,
# the inbound-mail path check, and people writing in (members and visitors). None of these may ever be flagged.
REAL_LEGITIMATE_NAMES = (
    "Server exception: Daemon failure: ses.inbound.mail.ingest.service (unit_failed, exit-code)",
    "Server exception: KeyError in website at /websocket.js",
    "Browser exception: Error in web.assets_frontend_lazy at /classifieds",
    "DX Spot Feed Freshness",
    "Backup Manager: hams_prod_pgbackrest_b2",
    "INC-AUTO",
    "Systemd Failed Services Tracker",
    "Inbound mail path check 2026-09-27 19:20 UTC",
    "AI safety flag: Official Observer on 14.074 MHz",
    "Demande de récupération de mot de passe",
    "App idea - support@hams.com",
    "WHATSAPP NUMBER - support@hams.com",
    "Hi support@hams.com",
    "Price?",
)


def newsletter_body():
    """HTML of a marketing newsletter: five nested layout tables, three images (one a 1x1 tracking pixel), footers."""
    return (
        "<html><body>"
        '<table width="100%"><tr><td><table width="600"><tr><td><table><tr><td><table><tr><td>'
        '<img src="https://img.example.net/hero.jpg" width="600" height="300">'
        "<h1>A gentle daily habit that people are talking about</h1>"
        "<p>Short promotional paragraph.</p>"
        '<a href="https://track.example.net/c/1">Learn more</a>'
        '<img src="https://img.example.net/banner.jpg" width="600" height="120">'
        "</td></tr></table></td></tr></table></td></tr></table></td></tr></table>"
        '<p><a href="https://track.example.net/view">View in browser</a> | '
        '<a href="https://track.example.net/u">Unsubscribe</a> | '
        '<a href="https://track.example.net/p">Update your preferences</a></p>'
        '<img src="https://track.example.net/open.gif" width="1" height="1">'
        "</body></html>"
    )


# What the Amazon SES receipt rule writes into a stored message (names and layout follow SES; values are invented).
SES_PASS_HEADERS = (
    "X-SES-Spam-Verdict: PASS\r\n"
    "X-SES-Virus-Verdict: PASS\r\n"
    "Authentication-Results: amazonses.com; spf=pass smtp.mailfrom=bounce.mailer.example.net; "
    "dkim=pass header.i=@mailer.example.net; dmarc=pass header.from=mailer.example.net\r\n"
)
BULK_HEADERS = (
    "Return-Path: <bounce-1234@bounce.mailer.example.net>\r\n"
    "List-Unsubscribe: <https://track.example.net/u>\r\n"
    "Precedence: bulk\r\n"
)


def raw_message(to_addr, subject, from_addr="news@mailer.example.net", body=None, headers="", html=True, message_id=None):
    """A raw RFC 822 message as bytes. ``headers`` is inserted verbatim before the From line."""
    content_type = "text/html" if html else "text/plain"
    body = newsletter_body() if body is None and html else (body or "A plain note.")
    message_id = message_id or "<fixture-%s@mailer.example.net>" % hashlib.sha256(f"{to_addr}|{subject}|{from_addr}".encode()).hexdigest()[:16]
    return (
        f"{headers}"
        f"From: {from_addr}\r\n"
        f"To: {to_addr}\r\n"
        f"Subject: {subject}\r\n"
        f"Message-ID: {message_id}\r\n"
        f"Content-Type: {content_type}; charset=utf-8\r\n"
        "\r\n"
        f"{body}\r\n"
    ).encode("utf-8")


class NotificationWatch:
    """Records everything a test transaction sends to a person after construction: queued emails (mail.mail), mail.notification
    rows (email and inbox), and bus messages (toasts, board refreshes). The bus is recorded, not suppressed-and-forgotten:
    every ``_sendone`` call is kept as (target, type)."""

    def __init__(self, test):
        env = test.env
        self.env = env
        self.bus = []

        def sendone(_bus, target, notification_type, *_args, **_kwargs):
            self.bus.append((target, notification_type))

        test.safe_patch_object(type(env["bus.bus"]), "_sendone", sendone, create=True)
        self.mail_floor = self._last("mail.mail")
        self.notification_floor = self._last("mail.notification")

    def _last(self, model):
        return self.env[model].search([], order="id desc", limit=1).id or 0

    def mails(self):
        return self.env["mail.mail"].search([("id", ">", self.mail_floor)], limit=1000)

    def notifications(self):
        return self.env["mail.notification"].search([("id", ">", self.notification_floor)], limit=1000)

    def count(self):
        """Everything that reached or would reach a person."""
        return len(self.mails()) + len(self.notifications()) + len(self.bus)
