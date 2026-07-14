# This software is distributed under the terms of the Affero General Public License (AGPL-3).

from odoo import http
from odoo.http import request
from odoo.addons.portal.controllers.portal import CustomerPortal, pager as portal_pager


class HelpdeskPortal(CustomerPortal):

    def _prepare_home_portal_values(self, counters):
        values = super()._prepare_home_portal_values(counters)
        if "ticket_count" in counters:
            utils = request.env["zero_sudo.security.utils"]
            svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")
            domain = [("partner_id", "=", request.env.user.partner_id.id)]
            if request.website:
                domain += [("website_id", "in", [False, request.website.id])]
            values["ticket_count"] = (
                request.env["hams_helpdesk.ticket"]
                .with_user(svc_uid)
                .with_company(request.website.company_id.id if getattr(request, 'website', None) else request.env.company.id)
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
        # [@ANCHOR: hams_helpdesk:multi_website_segregation]
        values = self._prepare_portal_layout_values()
        utils = request.env["zero_sudo.security.utils"]
        svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")
        Ticket = request.env["hams_helpdesk.ticket"].with_user(svc_uid).with_company(request.website.company_id.id if getattr(request, 'website', None) else request.env.company.id)

        domain = [("partner_id", "=", request.env.user.partner_id.id)]
        if request.website:
            domain += [("website_id", "in", [False, request.website.id])]

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

    @http.route(["/my/ticket/<int:ticket_id>"], type="http", auth="user", website=True)
    def portal_ticket_detail(self, ticket_id, **kw):
        utils = request.env["zero_sudo.security.utils"]
        svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")
        ticket_sudo = (
            request.env["hams_helpdesk.ticket"].with_user(svc_uid).with_company(request.website.company_id.id if getattr(request, 'website', None) else request.env.company.id).browse(ticket_id)
        )

        if (
            not ticket_sudo.exists()
            or ticket_sudo.partner_id != request.env.user.partner_id
        ):
            return request.redirect("/my")

        if (
            request.website
            and ticket_sudo.website_id
            and ticket_sudo.website_id != request.website
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
    def portal_ticket_close(self, ticket_id, **kw):
        utils = request.env["zero_sudo.security.utils"]
        svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")
        ticket_sudo = (
            request.env["hams_helpdesk.ticket"].with_user(svc_uid).with_company(request.website.company_id.id if getattr(request, 'website', None) else request.env.company.id).browse(ticket_id)
        )

        if (
            not ticket_sudo.exists()
            or ticket_sudo.partner_id != request.env.user.partner_id
        ):
            return request.redirect("/my")

        ticket_sudo.with_user(request.env.user).action_portal_close()
        return request.redirect("/my/ticket/%s" % ticket_id)

    @http.route(["/my/tickets/new"], type="http", auth="user", website=True)
    def portal_ticket_new(self, **kw):
        partner = request.env.user.partner_id

        # Exact Schema validation. Fail loudly if callsign isn't present
        callsign = partner.callsign
        if not callsign:
            import werkzeug
            raise werkzeug.exceptions.BadRequest("Callsign is required.")
        return request.render(
            "hams_helpdesk.portal_ticket_new",
            {
                "page_name": "ticket_new",
                "default_callsign": callsign,
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
    def portal_ticket_submit(self, name=None, description=None, callsign=None, **kw):
        # Verified by [@ANCHOR: hams_helpdesk:test_helpdesk_portal_tour]
        if not name:
            return request.redirect("/my/tickets/new")

        utils = request.env["zero_sudo.security.utils"]
        svc_uid = utils._get_service_uid("hams_helpdesk.user_helpdesk_service")

        company_id = (
            request.website.company_id.id
            if request.website
            else request.env.company.id
        )

        vals = {
            "name": name,
            "description": description,
            "callsign": callsign,
            "partner_id": request.env.user.partner_id.id,
            "website_id": request.website.id if request.website else False,
            "company_id": company_id,
        }
        clean_ctx = dict(request.env.context)
        clean_ctx.pop("prefetch_fields", None)
        ticket = request.env["hams_helpdesk.ticket"].with_context(**clean_ctx).with_user(svc_uid).with_company(company_id).create(vals)
        return request.redirect("/my/ticket/%s" % ticket.id)
