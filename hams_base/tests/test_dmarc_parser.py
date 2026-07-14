# -*- coding: utf-8 -*-
from odoo.tests import common, tagged
import base64
import zipfile
import io

@tagged('post_install', '-at_install')
class TestDmarcParser(common.TransactionCase):
    def setUp(self):
        super().setUp()
        self.DmarcReport = self.env['hams_base.dmarc.report']
        self.sample_xml = """<?xml version="1.0" encoding="UTF-8" ?>
<feedback>
  <report_metadata>
    <org_name>google.com</org_name>
    <email>noreply-dmarc-support@google.com</email>
    <report_id>12345</report_id>
    <date_range>
      <begin>1700000000</begin>
      <end>1700086400</end>
    </date_range>
  </report_metadata>
  <policy_published>
    <domain>hams.com</domain>
    <adkim>r</adkim>
    <aspf>r</aspf>
    <p>reject</p>
    <sp>reject</sp>
    <pct>100</pct>
  </policy_published>
  <record>
    <row>
      <source_ip>192.168.1.1</source_ip>
      <count>5</count>
      <policy_evaluated>
        <disposition>none</disposition>
        <dkim>pass</dkim>
        <spf>pass</spf>
      </policy_evaluated>
    </row>
  </record>
</feedback>"""

    def test_dmarc_xml_parsing(self):
        report = self.DmarcReport._parse_dmarc_xml(self.sample_xml)
        self.assertTrue(report)
        self.assertEqual(report.org_name, "google.com")
        self.assertEqual(report.domain, "hams.com")
        self.assertEqual(report.p, "reject")
        self.assertEqual(len(report.record_ids), 1)
        
        record = report.record_ids[0]
        self.assertEqual(record.source_ip, "192.168.1.1")
        self.assertEqual(record.count, 5)
        self.assertEqual(record.dkim_alignment, "pass")

    def test_message_new_with_zip(self):
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, 'w') as z:
            z.writestr('report.xml', self.sample_xml)
        
        zip_data = base64.b64encode(buf.getvalue())
        
        msg_dict = {
            'subject': 'DMARC Report',
            'attachments': [('report.zip', zip_data)]
        }
        
        report = self.DmarcReport.message_new(msg_dict)
        self.assertTrue(report)
        self.assertEqual(report.org_name, "google.com")
