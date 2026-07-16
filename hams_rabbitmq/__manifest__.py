{
    "name": "RabbitMQ Connection Pool",
    "summary": "Global RabbitMQ Connection Pool for all modules",
    "description": """
        Provides a global connection pool for RabbitMQ.
    """,
    "version": "1.0",
    "category": "Hidden",
    "depends": ["base", "zero_sudo"],
    "data": [
        "data/documentation.html",
    ],
    "external_dependencies": {
        "python": ["pika"],
    },
    "license": "AGPL-3",
}
