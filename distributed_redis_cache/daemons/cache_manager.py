#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
"""
Distributed Cache Manager Daemon
--------------------------------
A standalone asynchronous Python daemon designed to enforce cache phase coherence.
It listens for PostgreSQL 'distributed_cache_invalidation' NOTIFY events and
pushes them to the central Redis pub/sub queue.
"""

import os
import asyncio
import logging
import json
import asyncpg
import redis.asyncio as redis
from dotenv import load_dotenv

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s"
)
logger = logging.getLogger("cache_manager")

# --- Configuration ---
# [@ANCHOR: COMM_cache_manager_config]
ENV_FILE = "/opt/hams/etc/keys/cache_manager.env"
if os.path.exists(ENV_FILE):
    load_dotenv(ENV_FILE)

DB_HOST = os.getenv("DB_HOST", "localhost")
DB_PORT = os.getenv("DB_PORT", "5432")
DB_NAME = os.getenv("DB_NAME", "odoo")
DB_USER = os.getenv("DB_USER", "odoo")
DB_PASS = os.getenv("DB_PASS", "odoo")

# Use PGHOST if provided (e.g. for pgsock in VM)
if os.getenv("PGHOST"):
    DB_HOST = os.getenv("PGHOST")

REDIS_HOST = os.getenv("REDIS_HOST", os.getenv("redis_host", "localhost"))
REDIS_PORT = int(os.getenv("REDIS_PORT", os.getenv("redis_port", "6379")))
REDIS_PASS = os.getenv("REDIS_PASSWORD", os.getenv("redis_password"))

PG_CHANNEL = "distributed_cache_invalidation"
REDIS_CHANNEL = "odoo_cache_invalidation_bus"

redis_client = None


async def broadcast_to_redis(payload):
    # [@ANCHOR: COMM_cache_manager_redis_publish]
    """
    Pushes the invalidation payload to the central Redis bus
    for all active Odoo WSGI workers to intercept.
    """
    if not redis_client:
        return
    try:
        # Security: Validate JSON payload before publishing to Redis bus
        data = json.loads(payload)
        if not isinstance(data, dict) or not data.get("model") or not data.get("dbname"):
            logger.warning("Invalid payload received from Postgres: %s", payload)
            return

        async with redis_client.pipeline() as pipe:
            pipe.publish(REDIS_CHANNEL, payload)
            pipe.incr("global_cache_invalidation_counter")
            await pipe.execute()
        # SYSTEM OVERRIDE: Published invalidation to Redis: %s
        logger.info("Published invalidation to Redis: %s", payload)
    except json.JSONDecodeError:
        logger.error("Malformed JSON payload from Postgres: %s", payload)
    except Exception as e:  # audit-ignore-catch-all: # Tested by [@ANCHOR: COMM_test_cache_manager_exception_handling]
        logger.exception("Redis publish failed: %s", e)


_background_tasks = set()

def postgres_notify_handler(connection, pid, channel, payload):
    """
    Synchronous callback fired by asyncpg when a NOTIFY arrives.
    Schedules the Redis broadcast task on the asyncio event loop.
    """
    logger.info("Received Postgres NOTIFY on %s: %s", channel, payload)
    task = asyncio.create_task(broadcast_to_redis(payload))
    _background_tasks.add(task)
    task.add_done_callback(_background_tasks.discard)


async def main():
    global redis_client
    logger.info("Initializing Distributed Cache Manager Daemon...")

    # 1. Connect to Redis
    try:
        redis_client = redis.Redis(
            host=REDIS_HOST,
            port=REDIS_PORT,
            db=0,
            password=REDIS_PASS,
            decode_responses=True,
        )
        await redis_client.ping()
        logger.info("Connected to Redis at %s:%s", REDIS_HOST, REDIS_PORT)
    except redis.exceptions.RedisError as e:
        logger.critical("Fatal Redis connection error: %s", e)
        return

    # 2. Connect to PostgreSQL and LISTEN
    db_conns = []
    
    async def _reconnect():
        nonlocal db_conns
        for conn in db_conns:
            if not conn.is_closed():
                await conn.close()
        db_conns.clear()
        
        sys_conn = await asyncpg.connect(
            host=DB_HOST, port=DB_PORT, user=DB_USER, password=DB_PASS, database='postgres', timeout=10
        )
        query = """
            SELECT datname FROM pg_database 
            WHERE datistemplate = false 
            AND datname != 'postgres';
        """
        records = await sys_conn.fetch(query)
        dbs = [r['datname'] for r in records]
        # Also include the default DB_NAME just in case it wasn't caught
        if DB_NAME and DB_NAME not in dbs and DB_NAME != 'postgres':
            dbs.append(DB_NAME)
        await sys_conn.close()
        
        for db in dbs:
            try:
                conn = await asyncpg.connect(
                    host=DB_HOST, port=DB_PORT, user=DB_USER, password=DB_PASS, database=db, timeout=10
                )
                await conn.add_listener(PG_CHANNEL, postgres_notify_handler)
                db_conns.append(conn)
                logger.info("Listening to PostgreSQL channel '%s' on database '%s'...", PG_CHANNEL, db)
            except Exception as e:  # audit-ignore-catch-all: # Tested by [@ANCHOR: COMM_test_cache_manager_exception_handling]
                logger.exception("Could not connect to database %s: %s", db, e)

    while True:
        try:
            if not db_conns:
                await _reconnect()
                
            # Perform periodic health check on all connections
            for conn in list(db_conns):
                if conn.is_closed():
                    await _reconnect()
                    break
                await conn.execute("SELECT 1")
            
            await asyncio.sleep(60)  # audit-ignore-sleep: # Tested by [@ANCHOR: COMM_test_cache_manager_sleep]
        except asyncio.CancelledError:
            logger.info("Daemon shutting down cleanly.")
            break
        except Exception as e:  # audit-ignore-catch-all: # Tested by [@ANCHOR: COMM_test_cache_manager_exception_handling]
            logger.exception("PostgreSQL connection error: %s. Reconnecting in 5s...", e)
            try:
                await asyncio.sleep(5)  # audit-ignore-sleep: # Tested by [@ANCHOR: COMM_test_cache_manager_sleep]
                for conn in db_conns:
                    if not conn.is_closed():
                        await conn.close()
                db_conns.clear()
            except asyncio.CancelledError:
                break

    if redis_client:
        await redis_client.aclose()
    for conn in db_conns:
        if not conn.is_closed():
            await conn.close()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        logger.info("Daemon manually terminated.")
