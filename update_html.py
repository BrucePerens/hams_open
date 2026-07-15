import re

with open('database_management/data/documentation.html', 'r') as f:
    html = f.read()

replacements = [
    (r'<h2>1. Monitoring Table Health & Bloat</h2>', r'<h2 id="monitoring_table_health" data-trace="[@ANCHOR: COMM_bloat_alert_synergy]">1. Monitoring Table Health &amp; Bloat</h2>'),
    (r'<h3>How to reclaim space:</h3>', r'<h3 id="reclaim_space" data-trace="[@ANCHOR: COMM_vacuum_analyze]">How to reclaim space:</h3>'),
    (r'<h3>Index Advisor:</h3>', r'<h3 id="index_advisor" data-trace="[@ANCHOR: COMM_db_index_advisor]">Index Advisor:</h3>'),
    (r'<h2>2. Index Health & Usage</h2>', r'<h2 id="index_health_usage" data-trace="[@ANCHOR: COMM_db_index_stats]">2. Index Health &amp; Usage</h2>'),
    (r'<h2>3. Slow Query Tracking \(APM\)</h2>', r'<h2 id="slow_query_tracking" data-trace="[@ANCHOR: COMM_db_slow_queries] [@ANCHOR: COMM_db_explain_query]">3. Slow Query Tracking (APM)</h2>'),
    (r'<h2>4. Active Sessions & Kill Switch</h2>', r'<h2 id="active_sessions_kill" data-trace="[@ANCHOR: COMM_db_active_sessions] [@ANCHOR: COMM_db_terminate_backend]">4. Active Sessions &amp; Kill Switch</h2>'),
    (r'<h2>5. Performance Optimization Wizard</h2>', r'<h2 id="performance_optimization_wizard" data-trace="[@ANCHOR: COMM_pg_optimize_wizard] [@ANCHOR: COMM_db_settings_audit] [@ANCHOR: COMM_test_pg_config_views]">5. Performance Optimization Wizard</h2>'),
    (r'<h2>6. High Availability \(HA\) Orchestrator</h2>', r'<h2 id="ha_orchestrator" data-trace="[@ANCHOR: COMM_pg_ha_wizard]">6. High Availability (HA) Orchestrator</h2>'),
]

for pattern, repl in replacements:
    html = re.sub(pattern, repl, html)

with open('database_management/data/documentation.html', 'w') as f:
    f.write(html)
