/* SPDX-License-Identifier: AGPL-3.0-or-later */
/** @odoo-module **/
/* The pragma above only tells the asset-bundler lint this file was considered: this file is NOT in any asset bundle. It is served as-is
 * and loaded by a plain <script src> (classic script, globals on purpose), so Odoo never transpiles it and the pragma is inert. */

/*
 * [@ANCHOR: advertising:adsense_slot_push_js]
 *
 * Included once right after each <ins class="adsbygoogle"> slot (views/website_layout.xml), exactly
 * where the inline `(adsbygoogle = window.adsbygoogle || []).push({});` stood until 2026-10-03.
 * A browser executes every <script src> element, also when two share a URL, so each slot still
 * gets exactly one push. Moved out of the template for a strict CSP without 'unsafe-inline'.
 */
(window.adsbygoogle = window.adsbygoogle || []).push({});
