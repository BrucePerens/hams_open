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
import asyncpg
import redis.asyncio as redis

logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s"
)
logger = logging.getLogger("cache_manager")

# --- Configuration ---
# [@ANCHOR: cache_manager_config]
ENV_FILE = "/var/lib/odoo/daemon_keys/cache_manager.env"
if os.path.exists(ENV_FILE):
    try:
        from dotenv import load_dotenv
        load_dotenv(ENV_FILE)
    except ImportError:
        logger.warning("python-dotenv not installed, skipping .env loading")

DB_HOST = os.getenv("DB_HOST", "127.0.0.1")
DB_PORT = os.getenv("DB_PORT", "5432")
DB_NAME = os.getenv("DB_NAME", "odoo")
DB_USER = os.getenv("DB_USER", "odoo")
DB_PASS = os.getenv("DB_PASS", "odoo")

# Use PGHOST if provided (e.g. for pgsock in VM)
if os.getenv("PGHOST"):
    DB_HOST = os.getenv("PGHOST")

REDIS_HOST = os.getenv("REDIS_HOST", "127.0.0.1")
REDIS_PORT = int(os.getenv("REDIS_PORT", "6379"))
REDIS_PASS = os.getenv("REDIS_PASSWORD")

PG_CHANNEL = "distributed_cache_invalidation"
REDIS_CHANNEL = "odoo_cache_invalidation_bus"

redis_client = None


async def broadcast_to_redis(payload):
    # [@ANCHOR: cache_manager_redis_publish]
    """
    Pushes the invalidation payload to the central Redis bus
    for all active Odoo WSGI workers to intercept.
    """
    if not redis_client:
        return
    try:
        await redis_client.publish(REDIS_CHANNEL, payload)
        logger.debug(f"Published invalidation to Redis: {payload}")
    except redis.RedisError as e:
        logger.error(f"Redis publish failed: {e}")
    except Exception:
        logger.exception("Unexpected error during Redis publish")


def postgres_notify_handler(connection, pid, channel, payload):
    """
    Synchronous callback fired by asyncpg when a NOTIFY arrives.
    Schedules the Redis broadcast task on the asyncio event loop.
    """
    logger.info(f"Received Postgres NOTIFY on {channel}: {payload}")
    asyncio.create_task(broadcast_to_redis(payload))


async def main():
    global redis_client
    logger.info("Initializing Distributed Cache Manager Daemon...")

    # 1. Connect to Redis
    try:
        redis_client = redis.Redis(
            host=REDIS_HOST, port=REDIS_PORT, db=0, password=REDIS_PASS, decode_responses=True
        )
        await redis_client.ping()
        logger.info(f"Connected to Redis at {REDIS_HOST}:{REDIS_PORT}")
    except Exception as e:
        logger.critical(f"Fatal Redis connection error: {e}")
        return

    # 2. Connect to PostgreSQL and LISTEN
    while True:
        try:
            db_conn = await asyncpg.connect(
                host=DB_HOST,
                port=DB_PORT,
                user=DB_USER,
                password=DB_PASS,
                database=DB_NAME,
            )
            await db_conn.add_listener(PG_CHANNEL, postgres_notify_handler)
            logger.info(f"Listening to PostgreSQL channel '{PG_CHANNEL}'...")

            while not db_conn.is_closed():
                await asyncio.sleep(60)
        except asyncio.CancelledError:
            logger.info("Daemon shutting down cleanly.")
            break
        except (asyncpg.PostgresError, OSError) as e:
            logger.error(f"PostgreSQL connection error: {e}. Reconnecting in 5s...")
            await asyncio.sleep(5)
        except Exception:
            logger.exception("Unexpected error in PostgreSQL listener loop. Reconnecting in 5s...")
            await asyncio.sleep(5)

    if redis_client:
        await redis_client.aclose()
    if "db_conn" in locals() and not db_conn.is_closed():
        await db_conn.close()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        logger.info("Daemon manually terminated.")
