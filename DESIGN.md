# Desktop design

The Windows application uses a restrained storage-utility layout: navy navigation, warm neutral content, teal actions, system typography, fine borders and consistent spacing. The archive stages explain prepare, protect, verify and commit; progress uses observed counters rather than simulated percentages. Snapshots, accounts, transfers, recovery and preferences have explicit empty states. All destructive source removal is unavailable.

The Rust Dioxus interface delegates file dialogs, cryptography, storage and network work to blocking workers. English and Persian text are mapped in Rust; RTL uses logical CSS properties and isolated paths. System, light and dark themes are session preferences. CSS provides keyboard focus, enlarged readable form controls, a mobile navigation layout and reduced-motion behavior. Credentials and recovery secrets are rendered as text/password fields, never HTML; secrets are cleared when an operation finishes or after export. The recovery secret export and recovery-kit export are separate actions.

The layout adapts from two content columns to one, then from a sidebar to bottom navigation. The configured Windows window minimum is 760×540; 360px styles are provided as groundwork and have not established Android support.

This is an early development interface. Real screen-reader checks, 200% scaling, screenshots on all required platforms and representative-user tests remain required. Independent external design evaluation from a different model provider is unavailable in this session and has not been claimed.

`cargo +stable check -p cloudweave-desktop --offline` passed on the Windows development machine. This proves compilation only. The WebView rejects non-application navigation; provider authorization opens in the system browser. A bounded 64-event channel applies backpressure between workers and rendering. The interface reserves its completed state until the application service returns successfully, even if the lower-level engine has already finished its own commit stage.
# Runtime evidence — 2026-10-27

The built Windows application rendered in WebView2 on Windows 10.0.19045.6466. Reviewed screenshots: `artifacts/windows-overview-final.png` and `artifacts/windows-persian-settings-final.png`. Navigation, themes and Persian RTL were exercised in the actual local WebView through a temporary debug-only loopback CDP port. The native computer-use helper was unavailable; no native screen-reader or file-dialog workflow certification is claimed.

Automated axe-core 4.12.1 WCAG2 A/AA tags on the packaged English Overview and Persian Settings report zero violations (19/20 passes respectively), with decorative icon contrast left for manual review. The app now supplies an HTML document language and zoom-permitting viewport; controlled fixed scripts keep the document language/direction in sync with locale. These checks do not establish WCAG certification. Other key flow states, 360px/mobile, 200% scaling, all themes and five-person usability testing remain unfinished.
