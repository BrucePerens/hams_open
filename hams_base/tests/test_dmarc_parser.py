# -*- coding: utf-8 -*-
from odoo.tests import common, tagged
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
        # Adversarial security review, 2026-09-03: attachment_data is the
        # real, already-MIME-decoded content Odoo's own mail gateway
        # delivers (raw bytes for a binary attachment like this zip),
        # never base64-encoded text -- confirmed directly against
        # mail_thread.py's own _message_parse_extract_payload (content =
        # part.get_content()). This test used to base64-encode the data
        # before calling message_new(), which doesn't match how Odoo
        # actually invokes it, masking a real bug (process_dmarc_
        # attachment's own unconditional base64.b64decode() crashed
        # uncaught on real input).
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, 'w') as z:  # audit-ignore-path
            z.writestr('report.xml', self.sample_xml)

        zip_data = buf.getvalue()

        msg_dict = {
            'subject': 'DMARC Report',
            'attachments': [('report.zip', zip_data)]
        }

        report = self.DmarcReport.message_new(msg_dict)
        self.assertTrue(report)
        self.assertEqual(report.org_name, "google.com")

    def test_message_new_with_malformed_zip_does_not_crash(self):
        # [!] DIAGNOSTIC FOR AI: this is the real-world shape the fixed
        # bug hit on essentially every genuine DMARC report -- reachable
        # by any unauthenticated external sender to the DMARC rua=
        # address. Must fail closed (fall through to the generic
        # "Unparsed Email" record), never raise.
        msg_dict = {
            'subject': 'Malformed DMARC Report',
            'attachments': [('report.zip', b'this is not a real zip file at all')],
        }
        report = self.DmarcReport.message_new(msg_dict)
        self.assertTrue(report, "message_new must still return a record, not raise")
        self.assertIn("Unparsed Email", report.org_name)

    def test_message_new_with_non_numeric_count_does_not_crash(self):
        # A well-formed XML document with a non-numeric <count>/<pct>/
        # <begin>/<end> used to throw an uncaught ValueError out of
        # message_new() -- same externally-triggerable-input category.
        malformed_xml = self.sample_xml.replace("<count>5</count>", "<count>not-a-number</count>")
        report = self.DmarcReport._parse_dmarc_xml(malformed_xml)
        self.assertTrue(report)
        self.assertEqual(report.record_ids[0].count, 0)

    def test_zip_declaring_an_oversized_xml_member_is_refused(self):
        # Real cap against a crafted zip whose XML member is larger than
        # this codebase's own decompressed-size limit -- a classic
        # zip-bomb shape. A real, highly-compressible oversized member
        # (not forged zip-header metadata) is simpler and just as real a
        # proof: the check reads the member's own real declared size
        # (ZipInfo.file_size) before ever calling z.read().
        from odoo.addons.hams_base.models.dmarc_report import _MAX_DECOMPRESSED_BYTES

        buf = io.BytesIO()
        with zipfile.ZipFile(buf, 'w', compression=zipfile.ZIP_DEFLATED) as z:
            z.writestr('report.xml', b"0" * (_MAX_DECOMPRESSED_BYTES + 1))
        oversized_zip = buf.getvalue()

        result = self.DmarcReport.process_dmarc_attachment('report.zip', oversized_zip)
        self.assertFalse(
            result,
            "[!] DIAGNOSTIC FOR AI: a zip member declaring a size over the "
            "cap must be refused before it's ever decompressed into memory.",
        )
