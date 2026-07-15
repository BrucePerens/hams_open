# -*- coding: utf-8 -*-
from odoo import models, fields, api
import logging
import base64
import zipfile
import gzip
import io
import xml.etree.ElementTree as ET

_logger = logging.getLogger(__name__)

class DmarcReport(models.Model):
    _name = "hams_base.dmarc.report"
    _description = "DMARC RUA Report"
    _inherit = ["mail.thread"]
    _order = "date_range_begin desc"

    name = fields.Char(string="Name", default="New Report")
    org_name = fields.Char(string="Organization")
    email = fields.Char(string="Contact Email")
    report_id = fields.Char(string="Report ID", index=True)
    date_range_begin = fields.Datetime(string="Begin Date")
    date_range_end = fields.Datetime(string="End Date")
    domain = fields.Char(string="Domain", index=True)
    
    adkim = fields.Char(string="ADKIM")
    aspf = fields.Char(string="ASPF")
    p = fields.Char(string="Policy (p)")
    sp = fields.Char(string="Subdomain Policy (sp)")
    pct = fields.Integer(string="Percentage")

    record_ids = fields.One2many("hams_base.dmarc.record", "report_id", string="Records")

    @api.model
    def message_new(self, msg_dict, custom_values=None):
        """
        Invoked by Odoo when a new email arrives to the configured alias.
        We intercept the attachments and parse them into DMARC reports.
        """
        attachments = msg_dict.get('attachments', [])
        report = self.env['hams_base.dmarc.report']
        
        for attachment in attachments:
            name, data = attachment
            parsed_report = self.process_dmarc_attachment(name, data)
            if parsed_report and not report:
                report = parsed_report
        
        if report:
            report.message_post(
                body=msg_dict.get('body', ''),
                subject=msg_dict.get('subject', ''),
                author_id=msg_dict.get('author_id'),
                email_from=msg_dict.get('email_from'),
                message_type='email',
                subtype_xmlid='mail.mt_comment',
            )
            return report

        vals = {'org_name': "Unparsed Email: " + msg_dict.get('subject', 'No Subject')}
        if custom_values:
            vals.update(custom_values)
        return super().message_new(msg_dict, custom_values=vals)

    @api.model
    def process_dmarc_attachment(self, attachment_name, attachment_data):
        """
        Parses a DMARC XML report (could be raw XML, ZIP, or GZIP).
        attachment_data is base64 encoded.
        """
        raw_data = base64.b64decode(attachment_data)
        xml_content = None

        try:
            if attachment_name.endswith('.zip'):
                with zipfile.ZipFile(io.BytesIO(raw_data)) as z:  # audit-ignore-path
                    for name in z.namelist():
                        if name.endswith('.xml'):
                            xml_content = z.read(name)
                            break
            elif attachment_name.endswith('.gz'):
                xml_content = gzip.decompress(raw_data)
            elif attachment_name.endswith('.xml'):
                xml_content = raw_data
        except Exception as e:  # audit-ignore-catch-all
            _logger.exception("Failed to extract DMARC report from %s: %s", attachment_name, e)
            return False

        if not xml_content:
            return False

        return self._parse_dmarc_xml(xml_content)

    @api.model
    def _parse_dmarc_xml(self, xml_content):
        try:
            root = ET.fromstring(xml_content)
        except ET.ParseError as e:
            _logger.warning("Failed to parse DMARC XML: %s", e)
            return False

        report_metadata = root.find("report_metadata")
        policy_published = root.find("policy_published")

        if report_metadata is None or policy_published is None:
            return False

        org_name = report_metadata.findtext("org_name")
        email = report_metadata.findtext("email")
        report_id = report_metadata.findtext("report_id")
        
        # Date range
        date_range = report_metadata.find("date_range")
        begin = int(date_range.findtext("begin")) if date_range is not None else 0
        end = int(date_range.findtext("end")) if date_range is not None else 0

        # Check if report already exists
        existing = self.env['hams_base.dmarc.report'].search([('report_id', '=', report_id)], limit=1)
        if existing:
            return existing

        domain = policy_published.findtext("domain")
        
        report_vals = {
            "org_name": org_name,
            "email": email,
            "report_id": report_id,
            "date_range_begin": fields.Datetime.from_timestamp(begin) if begin else False,
            "date_range_end": fields.Datetime.from_timestamp(end) if end else False,
            "domain": domain,
            "adkim": policy_published.findtext("adkim"),
            "aspf": policy_published.findtext("aspf"),
            "p": policy_published.findtext("p"),
            "sp": policy_published.findtext("sp"),
            "pct": int(policy_published.findtext("pct") or 100),
        }

        report = self.env['hams_base.dmarc.report'].create(report_vals)

        record_vals_list = []
        for record in root.findall("record"):
            row = record.find("row")
            source_ip = row.findtext("source_ip") if row is not None else False
            count = int(row.findtext("count") or 0) if row is not None else 0
            
            policy_evaluated = row.find("policy_evaluated") if row is not None else None
            disposition = policy_evaluated.findtext("disposition") if policy_evaluated is not None else False
            dkim_pass = policy_evaluated.findtext("dkim") if policy_evaluated is not None else False
            spf_pass = policy_evaluated.findtext("spf") if policy_evaluated is not None else False

            record_vals_list.append({
                "report_id": report.id,
                "source_ip": source_ip,
                "count": count,
                "disposition": disposition,
                "dkim_alignment": dkim_pass,
                "spf_alignment": spf_pass,
            })

        if record_vals_list:
            self.env['hams_base.dmarc.record'].create(record_vals_list)

        return report

class DmarcRecord(models.Model):
    _name = "hams_base.dmarc.record"
    _description = "DMARC RUA Record Details"

    name = fields.Char(string="Name", default="Record")
    report_id = fields.Many2one("hams_base.dmarc.report", string="Report", required=True, ondelete="cascade")
    source_ip = fields.Char(string="Source IP", index=True)
    count = fields.Integer(string="Message Count")
    disposition = fields.Char(string="Disposition (none, quarantine, reject)")
    dkim_alignment = fields.Char(string="DKIM Alignment (pass/fail)")
    spf_alignment = fields.Char(string="SPF Alignment (pass/fail)")
