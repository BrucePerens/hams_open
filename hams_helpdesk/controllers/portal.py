# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo import http
from odoo.http import request
from odoo.addons.portal.controllers.portal import CustomerPortal, pager as portal_pager


class HelpdeskPortal(CustomerPortal):

    # [@ANCHOR: hams_helpdesk:COMM_prepare_home_portal_values]
    def _prepare_home_portal_values(self, counters):
        values = super()._prepare_home_portal_values(counters)
        if "ticket_count" in counters:
            utils = request.env["zero_sudo.security.utils"]
            svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")
            domain = [("partner_id", "=", request.env.user.partner_id.id)]
            try:
                req_website = request.website
            except (AttributeError,):
                req_website = False

            company_id = req_website.company_id.id if req_website else request.env.company.id
            if req_website:
                domain += [("website_id", "in", [False, req_website.id])]
            
            values["ticket_count"] = (
                request.env["hams_helpdesk.ticket"]
                .with_user(svc_uid)
                .with_company(company_id)
                .search_count(domain)
            )
        return values

    @http.route(
        ["/my/tickets", "/my/tickets/page/<int:page>"],
        type="http",
        auth="user",
        website=True,
    )
    def portal_my_tickets(self, page=1, **kw):
        # Verified by [@ANCHOR: test_06_multi_website_awareness_logic]
        values = self._prepare_portal_layout_values()
        utils = request.env["zero_sudo.security.utils"]
        svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")
        try:
            req_website = request.website
        except (AttributeError,):
            req_website = False
            
        company_id = req_website.company_id.id if req_website else request.env.company.id
        Ticket = request.env["hams_helpdesk.ticket"].with_user(svc_uid).with_company(company_id)

        domain = [("partner_id", "=", request.env.user.partner_id.id)]
        if req_website:
            domain += [("website_id", "in", [False, req_website.id])]

        ticket_count = Ticket.search_count(domain)
        pager = portal_pager(url="/my/tickets", total=ticket_count, page=page, step=20)
        tickets = Ticket.search(
            domain, limit=20, offset=pager["offset"], order="create_date desc"
        )

        values.update(
            {
                "tickets": tickets,
                "page_name": "ticket",
                "pager": pager,
                "default_url": "/my/tickets",
            }
        )
        return request.render("hams_helpdesk.portal_my_tickets", values)

    # [@ANCHOR: hams_helpdesk:COMM_portal_ticket_detail]
    @http.route(["/my/ticket/<int:ticket_id>"], type="http", auth="user", website=True)
    def portal_ticket_detail(self, ticket_id, **kw):
        utils = request.env["zero_sudo.security.utils"]
        svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")
        try:
            req_website = request.website
        except (AttributeError,):
            req_website = False
            
        company_id = req_website.company_id.id if req_website else request.env.company.id
        ticket_sudo = (
            request.env["hams_helpdesk.ticket"].with_user(svc_uid).with_company(company_id).browse(ticket_id)
        )

        if (
            not ticket_sudo.exists()
            or ticket_sudo.partner_id != request.env.user.partner_id
        ):
            return request.redirect("/my")

        if (
            req_website
            and ticket_sudo.website_id
            and ticket_sudo.website_id != req_website
        ):
            return request.redirect("/my")

        values = {
            "ticket": ticket_sudo.with_user(request.env.user),
            "page_name": "ticket_detail",
        }
        return request.render("hams_helpdesk.portal_ticket_detail", values)

    @http.route(
        ["/my/ticket/<int:ticket_id>/close"],
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    # [@ANCHOR: hams_helpdesk:COMM_portal_ticket_close]
    def portal_ticket_close(self, ticket_id, **kw):
        utils = request.env["zero_sudo.security.utils"]
        svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")
        try:
            req_website = request.website
        except (AttributeError,):
            req_website = False
            
        company_id = req_website.company_id.id if req_website else request.env.company.id
        ticket_sudo = (
            request.env["hams_helpdesk.ticket"].with_user(svc_uid).with_company(company_id).browse(ticket_id)
        )

        if (
            not ticket_sudo.exists()
            or ticket_sudo.partner_id != request.env.user.partner_id
        ):
            return request.redirect("/my")

        ticket_sudo.with_user(request.env.user).action_portal_close()
        return request.redirect("/my/ticket/%s" % ticket_id)

    @http.route(["/my/tickets/new"], type="http", auth="user", website=True)
    # [@ANCHOR: hams_helpdesk:COMM_portal_ticket_new]
    def portal_ticket_new(self, **kw):
        # Found live 2026-08-29 as a Prospective Ham/SWL persona (a real,
        # site-offered signup option specifically for users studying for
        # their license, i.e. by definition without a callsign yet): this
        # used to raise a raw 400 Bad Request ("Callsign is required.")
        # for any user without one, with no way back. That contradicted
        # the rest of this same flow -- the form template's own callsign
        # field is labeled "Your callsign (if applicable)", and
        # portal_ticket_submit()/helpdesk_ticket.create() both already
        # handle a missing callsign gracefully (fall back to empty, no
        # required=True on the model field). Nothing downstream needed
        # this check; just stopped enforcing it.
        callsign = request.env.user.partner_id.callsign
        # A caller (e.g. ham_shack/data/local_relay_guide.html's own "report a bug" link)
        # can deep-link straight into a specific category via ?ticket_type=hams_local_relay.
        # Validated against the model's own real selection values here, not trusted blindly
        # from a query string -- an unrecognized value falls back to the field's own
        # "general" default rather than being passed through to the template/create() call.
        valid_types = dict(
            request.env["hams_helpdesk.ticket"]._fields["ticket_type"].selection
        )
        requested_type = kw.get("ticket_type")
        default_ticket_type = requested_type if requested_type in valid_types else "general"
        return request.render(
            "hams_helpdesk.portal_ticket_new",
            {
                "page_name": "ticket_new",
                "default_callsign": callsign,
                "default_ticket_type": default_ticket_type,
                "ticket_type_selection": request.env["hams_helpdesk.ticket"]._fields["ticket_type"].selection,
            },
        )

    @http.route(
        ["/my/tickets/submit"],
        type="http",
        auth="user",
        methods=["POST"],
        website=True,
        csrf=True,
    )
    def portal_ticket_submit(self, name=None, description=None, callsign=None, ticket_type=None, **kw):
        # Verified by [@ANCHOR: helpdesk_portal_tour]
        if not name:
            return request.redirect("/my/tickets/new")

        utils = request.env["zero_sudo.security.utils"]
        svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")

        # Same real-selection-value validation as portal_ticket_new's own GET handler --
        # a portal POST body is caller-controlled, so re-check here too rather than
        # trusting the hidden form field wasn't tampered with; an unrecognized value
        # falls back to the model field's own "general" default.
        valid_ticket_types = dict(
            request.env["hams_helpdesk.ticket"]._fields["ticket_type"].selection
        )
        if ticket_type not in valid_ticket_types:
            ticket_type = "general"

        try:
            req_website = request.website
        except (AttributeError,):
            req_website = False

        company_id = (
            req_website.company_id.id
            if req_website
            else request.env.company.id
        )

        vals = {
            "name": name,
            "description": description,
            "callsign": callsign or request.env.user.partner_id.callsign,
            "ticket_type": ticket_type,
            "partner_id": request.env.user.partner_id.id,
            "website_id": req_website.id if req_website else False,
            "company_id": company_id,
        }
        clean_ctx = dict(request.env.context)
        clean_ctx.pop("prefetch_fields", None)
        ticket = request.env["hams_helpdesk.ticket"].with_context(**clean_ctx).with_user(svc_uid).with_company(company_id).create(vals)
        return request.redirect("/my/ticket/%s" % ticket.id)
