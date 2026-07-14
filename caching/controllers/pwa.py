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
            "background_color": "#ffffff",
            "theme_color": "#875A7B",
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
        html = """
        <!DOCTYPE html>
        <html>
        <head>
            <title>Offline</title>
            <meta name="viewport" content="width=device-width, initial-scale=1">
            <style>
                body { font-family: sans-serif; text-align: center; padding: 2em; }
            </style>
        </head>
        <body>
            <h1>You are offline</h1>
            <p>Please check your internet connection and try again.</p>
        </body>
        </html>
        """
        return request.make_response(html)
