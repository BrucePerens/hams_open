# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import os
import logging
import redis

_logger = logging.getLogger(__name__)

# [@ANCHOR: redis_connection_pool]
redis_host = os.getenv("REDIS_HOST") or "redis"
redis_port = int(os.getenv("REDIS_PORT") or "6379")
redis_password = os.getenv("REDIS_PASSWORD")
redis_pool = redis.ConnectionPool(
    host=redis_host,
    port=redis_port,
    password=redis_password,
    db=0,
    decode_responses=True,
    socket_timeout=1.0,
    socket_connect_timeout=1.0,
)
_logger.debug("Initialized Centralized Redis Connection Pool.")
