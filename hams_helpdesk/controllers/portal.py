from odoo import http
from odoo.http import request
from odoo.addons.portal.controllers.portal import CustomerPortal, pager as portal_pager

class HelpdeskPortal(CustomerPortal):

    def _prepare_home_portal_values(self, counters):
        values = super()._prepare_home_portal_values(counters)
        if "ticket_count" in counters:
            domain = [("partner_id", "=", request.env.user.partner_id.id)]
            if request.website:
                domain += [("website_id", "in", [False, request.website.id])]
            values["ticket_count"] = request.env["hams_helpdesk.ticket"].search_count(domain)
        return values

    @http.route(["/my/tickets", "/my/tickets/page/<int:page>"], type="http", auth="user", website=True)
    def portal_my_tickets(self, page=1, **kw):
        values = self._prepare_portal_layout_values()
        Ticket = request.env["hams_helpdesk.ticket"]
        domain = [("partner_id", "=", request.env.user.partner_id.id)]
        if request.website:
            domain += [("website_id", "in", [False, request.website.id])]

        ticket_count = Ticket.search_count(domain)
        pager = portal_pager(
            url="/my/tickets",
            total=ticket_count,
            page=page,
            step=20
        )
        tickets = Ticket.search(domain, limit=20, offset=pager["offset"], order="create_date desc")

        values.update({
            "tickets": tickets,
            "page_name": "ticket",
            "pager": pager,
            "default_url": "/my/tickets",
        })
        return request.render("hams_helpdesk.portal_my_tickets", values)

    @http.route(["/my/ticket/<int:ticket_id>"], type="http", auth="user", website=True)
    def portal_ticket_detail(self, ticket_id, **kw):
        ticket = request.env["hams_helpdesk.ticket"].browse(ticket_id)
        if not ticket.exists() or ticket.partner_id != request.env.user.partner_id:
            return request.redirect("/my")

        if request.website and ticket.website_id and ticket.website_id != request.website:
            return request.redirect("/my")

        values = {
            "ticket": ticket,
            "page_name": "ticket_detail",
        }
        return request.render("hams_helpdesk.portal_ticket_detail", values)
