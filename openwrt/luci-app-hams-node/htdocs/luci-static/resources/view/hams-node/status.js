// SPDX-License-Identifier: GPL-2.0-or-later
'use strict';
'require view';
'require form';
'require rpc';
'require poll';
'require ui';

var callStatus = rpc.declare({ object: 'hams-node', method: 'status' });
var callEnroll = rpc.declare({ object: 'hams-node', method: 'enroll_start' });

function row(label, value) {
	return E('tr', { 'class': 'tr' }, [ E('td', { 'class': 'td left', 'width': '33%' }, label), E('td', { 'class': 'td left' }, value) ]);
}

function draw(box, s) {
	var b = s.boundary || {};
	var rows = [
		row(_('Enrolment'), s.enrolled ? _('Enrolled') : (s.state || _('Not enrolled'))),
		row(_('Delegated /60'), s.prefix60 || '-'),
		row(_('.ham name'), s.ham_name || '-'),
		row(_('Part 97 boundary'), b.active ? _('Active') : _('Not loaded')),
		row(_('Refused, encrypted'), String(b.refused_encrypted || 0)),
		row(_('Refused, other'), String(b.refused_other || 0)),
		row(_('Ham-side traffic'), b.integrity_only ? _('Integrity-only (authenticated, not encrypted)') : '-')
	];
	if (s.code)
		rows.splice(1, 0, row(_('Enrol code'), E('strong', {}, s.code)));
	var peers = (s.peers || []).map(function(p) {
		return E('tr', { 'class': 'tr' }, [ E('td', { 'class': 'td' }, p.prefix), E('td', { 'class': 'td' }, p.endpoint), E('td', { 'class': 'td' }, p.key) ]);
	});
	if (!peers.length)
		peers = [ E('tr', { 'class': 'tr placeholder' }, E('td', { 'class': 'td', 'colspan': 3 }, E('em', _('No active WireGuard peers.')))) ];
	var enroll = s.enrolled ? '' : E('button', {
		'class': 'btn cbi-button cbi-button-action',
		'click': ui.createHandlerFn(this, function() { return callEnroll(); })
	}, _('Enrol with hams.com'));
	box.replaceChildren(
		E('div', { 'class': 'cbi-section' }, [ E('h3', _('Status')), E('table', { 'class': 'table' }, rows), enroll ]),
		E('div', { 'class': 'cbi-section' }, [ E('h3', _('Active WireGuard peers')), E('table', { 'class': 'table' }, [
			E('tr', { 'class': 'tr table-titles' }, [ E('th', { 'class': 'th' }, _('Member /60')), E('th', { 'class': 'th' }, _('Endpoint')), E('th', { 'class': 'th' }, _('Public key')) ])
		].concat(peers)) ]));
}

return view.extend({
	load: function() { return callStatus(); },

	render: function(status) {
		var box = E('div');
		draw(box, status);
		poll.add(function() { return callStatus().then(function(s) { draw(box, s); }); }, 5);

		var m = new form.Map('hams_node_ondemand', _('hams.com Net'), _('Node status and settings.'));
		var s = m.section(form.NamedSection, 'main', 'ondemand', _('Settings'));
		s.addremove = false;
		var o = s.option(form.Flag, 'enabled', _('Enable on-demand tunnels'));
		o.rmempty = false;
		o = s.option(form.ListValue, 'link_type', _('Uplink type'), _('Declare an RF link here: WireGuard is encrypted and is never used on one (47 CFR 97.113).'));
		o.value('internet', _('Internet'));
		o.value('rf', _('RF (no WireGuard)'));
		o.default = 'internet';
		return m.render().then(function(node) { return E([ box, node ]); });
	}
});
