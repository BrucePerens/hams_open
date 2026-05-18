# -*- coding: utf-8 -*-
import psycopg2
from odoo.addons.hams_test.tests import real_transaction

# Monkeypatch hams_test to fix missing import bug in its tearDown
real_transaction.psycopg2 = psycopg2

from . import test_backup
from . import test_backup_security
from . import test_tour

# Monkeypatch hams_test to fix missing import bug in its tearDown
real_transaction.psycopg2 = psycopg2
