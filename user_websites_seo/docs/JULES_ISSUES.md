## 2026-05-19 23:59 - user_websites_seo
- Encountered multiple timeouts when running `test_runner.py --provision-jules`. It seems the environment setup takes longer than the tool's timeout.
- RabbitMQ service failed to start during provisioning: `Job for rabbitmq-server.service failed because the control process exited with error code.`.
- `test_runner.py` reported SUCCESS for Semantic Anchors even when it failed later due to DB connection issues.
- Running all tests for `user_websites` timed out.
