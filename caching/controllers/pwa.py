# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
import json
from odoo import http
from odoo.http import request


class PWAController(http.Controller):

    @http.route('/manifest.json', type='http', auth='public', website=True, sitemap=False)
    def manifest(self):
        website = request.website or request.env['website'].get_current_website()
        app_name = website.name if website else 'Odoo PWA'
        
        manifest_data = {
            "name": app_name,
            "short_name": app_name,
            "start_url": "/",
            "display": "standalone",
            "background_color": website.caching_pwa_background_color if website and hasattr(website, 'caching_pwa_background_color') else "#ffffff",
            "theme_color": website.caching_pwa_theme_color if website and hasattr(website, 'caching_pwa_theme_color') else "#875A7B",
            "icons": [
                {
                    "src": "/web/static/img/logo.png",
                    "sizes": "192x192",
                    "type": "image/png"
                }
            ]
        }
        headers = [('Content-Type', 'application/json')]
        return request.make_response(json.dumps(manifest_data), headers=headers)

    @http.route('/offline', type='http', auth='public', website=True, sitemap=False)
    def offline(self):
        return request.render('caching.pwa_offline_template')
