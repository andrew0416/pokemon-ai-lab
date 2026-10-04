# S23 phase cost and clean controls

Parent: S22 `70d5b0b463dc2cab2deb365c67ffcb2e2e7f95b8`.
Only diagnostic instrumentation and benchmark controls; no search algorithm change.
Instrumentation is compiled out of the speed executable. All production/default
features and portable scalar paths are retained. Local runs check logic only;
GitHub Actions supplies performance evidence. Existing S22 evidence is frozen.
