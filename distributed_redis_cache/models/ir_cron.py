# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, models, tools

from odoo.addons.distributed_redis_cache.redis_cache import poll_and_clear_local_cache


class IrCron(models.Model):
    _inherit = "ir.cron"

    @classmethod
    def _process_job(cls, cron_cr, job):
        # [@ANCHOR: distributed_redis_cache:COMM_cron_cache_interceptor]
        """Runs the same poll-and-clear `ir.http._authenticate` performs, before
        a cron job's own callback can reach a `@distributed_cache()`-decorated
        method.

        `ir.http._authenticate` is Odoo's HTTP auth hook -- it is never reached
        from cron dispatch (confirmed against the installed Odoo source:
        `odoo/service/server.py`'s `WorkerCron.process_work()`, in real
        multi-worker mode, and its single-process `cron%d` thread both call
        `IrCron._process_jobs(db_name)` by importing `ir.cron`'s base module
        class directly, bypassing this registry entirely). Without this
        override, a worker process that only ever runs cron jobs -- never an
        HTTP request -- would keep serving its L1 `_local_cache` entries
        unconditionally for its whole lifetime, regardless of any real-time
        invalidation.

        This method IS reached from both dispatch modes: `_process_jobs`'s own
        `_process_jobs_loop` (shared by both) deliberately routes the per-job
        call through `Registry(db_name)[IrCron._name]._process_job(...)`
        rather than a raw class reference, specifically "to take into account
        overridings of _process_job() on that database" (its own comment) --
        this is the one cron-dispatch point every `_inherit`-based override
        actually reaches, in single-process and multi-worker mode alike.
        """
        init_mode = tools.config.get("init")
        update_mode = tools.config.get("update")
        stop_after_init = tools.config.get("stop_after_init")

        if not (init_mode or update_mode or stop_after_init):
            # `cls` here is the registry-composed model CLASS, not a
            # recordset instance, so it has no `.env` of its own -- build a
            # throwaway one the same way the base implementation does for
            # its own job dispatch (job['user_id']).
            env = api.Environment(cron_cr, job["user_id"], {})
            poll_and_clear_local_cache(env)

        return super()._process_job(cron_cr, job)
