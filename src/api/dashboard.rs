//! Embedded web dashboard for HiveBox.
//!
//! Serves a single-page management UI at `/` that talks to the REST API
//! on the same origin. No external dependencies — everything is inlined.

use axum::response::Html;

/// Serves the dashboard HTML page.
pub async fn dashboard_page() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

const DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>HiveBox</title>
<style>
:root{
  --bg:#09090b;--bg2:#18181b;--bg3:#27272a;--border:#3f3f46;
  --fg:#fafafa;--fg2:#a1a1aa;--fg3:#71717a;
  --pri:#eac01b;--pri2:#c9a417;--pri3:#92400e;--pri-bg:rgba(234,192,27,.08);
  --green:#22c55e;--green-bg:#052e16;
  --red:#ef4444;--red-bg:#450a0a;
  --blue:#3b82f6;--blue-bg:#172554;
  --purple:#a78bfa;
  --radius:8px;--radius-lg:12px;
  --shadow:0 4px 24px rgba(0,0,0,.3);
}
html.light{
  --bg:#f8f9fa;--bg2:#ffffff;--bg3:#f1f3f5;--border:#dee2e6;
  --fg:#1a1a1a;--fg2:#6b7280;--fg3:#9ca3af;
  --pri:#c9a417;--pri2:#a88a10;--pri3:#fef3c7;--pri-bg:rgba(201,164,23,.06);
  --green:#16a34a;--green-bg:#dcfce7;
  --red:#dc2626;--red-bg:#fee2e2;
  --blue:#2563eb;--blue-bg:#dbeafe;
  --shadow:0 4px 24px rgba(0,0,0,.08);
}
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:'Inter',-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:var(--bg);color:var(--fg);min-height:100vh;-webkit-font-smoothing:antialiased;transition:background .2s,color .2s}

/* Login */
#login-screen{display:flex;align-items:center;justify-content:center;min-height:100vh}
.login-box{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius-lg);padding:48px;width:400px;box-shadow:var(--shadow)}
.logo{text-align:center;margin-bottom:8px}
.logo svg{display:block;margin:0 auto;height:36px;width:auto}
.login-sub{font-size:14px;color:var(--fg3);margin:6px 0 32px;text-align:center}
.field{margin-bottom:20px}
.field label{display:block;font-size:13px;font-weight:500;color:var(--fg2);margin-bottom:6px}
.field input,.field select,.field textarea{width:100%;padding:10px 14px;background:var(--bg);border:1px solid var(--border);border-radius:var(--radius);color:var(--fg);font-size:14px;font-family:inherit;transition:border .15s;box-sizing:border-box}
.field input:focus,.field select:focus,.field textarea:focus{outline:none;border-color:var(--pri)}
.field input::placeholder,.field textarea::placeholder{color:var(--fg3)}
.field textarea{font-family:'JetBrains Mono','Fira Code',monospace;font-size:12px;resize:vertical}
.btn-primary{width:100%;padding:12px;background:var(--pri);color:#000;border:none;border-radius:var(--radius);font-size:14px;font-weight:600;cursor:pointer;transition:background .15s}
.btn-primary:hover{background:var(--pri2)}
.login-err{color:var(--red);font-size:13px;margin-top:12px;display:none}

/* App */
#app{display:none}
nav{background:var(--bg2);border-bottom:1px solid var(--border);padding:0 32px;height:56px;display:flex;align-items:center;justify-content:space-between;position:sticky;top:0;z-index:50}
.nav-brand{display:flex;align-items:center}
.nav-brand svg{height:28px;width:auto}
.nav-right{display:flex;align-items:center;gap:12px}
.status-dot{width:8px;height:8px;border-radius:50%;background:var(--green);display:inline-block;margin-right:6px}
.nav-status{font-size:13px;color:var(--fg3)}
.btn-icon{width:36px;height:36px;display:flex;align-items:center;justify-content:center;background:transparent;border:1px solid var(--border);border-radius:var(--radius);color:var(--fg2);cursor:pointer;font-size:16px;transition:all .15s}
.btn-icon:hover{background:var(--bg3);color:var(--fg)}
.btn-ghost{padding:8px 16px;background:transparent;border:1px solid var(--border);border-radius:var(--radius);color:var(--fg2);cursor:pointer;font-size:13px;font-family:inherit;transition:all .15s}
.btn-ghost:hover{background:var(--bg3);color:var(--fg)}

main{max-width:1280px;margin:0 auto;padding:32px}

/* Tabs */
.tabs{display:flex;gap:4px;margin-bottom:28px;border-bottom:2px solid var(--border);padding-bottom:0}
.tab{padding:10px 24px;font-size:14px;font-weight:500;color:var(--fg3);cursor:pointer;border:none;background:none;font-family:inherit;border-bottom:2px solid transparent;margin-bottom:-2px;transition:all .15s}
.tab:hover{color:var(--fg)}
.tab.active{color:var(--pri);border-bottom-color:var(--pri);font-weight:600}
.tab-content{display:none}
.tab-content.active{display:block}

/* Stats cards row */
.stats{display:grid;grid-template-columns:repeat(4,1fr);gap:16px;margin-bottom:28px}
.stat-card{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius-lg);padding:20px}
.stat-label{font-size:12px;font-weight:500;text-transform:uppercase;letter-spacing:.5px;color:var(--fg3);margin-bottom:8px}
.stat-value{font-size:28px;font-weight:700;color:var(--fg);letter-spacing:-1px}
.stat-value .unit{font-size:14px;font-weight:400;color:var(--fg3)}

/* Charts */
.charts{display:grid;grid-template-columns:1fr 1fr;gap:16px;margin-bottom:28px}
.chart-card{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius-lg);padding:20px}
.chart-title{font-size:13px;font-weight:600;margin-bottom:16px;display:flex;align-items:center;justify-content:space-between}
.chart-title .live{font-size:11px;color:var(--green);font-weight:500}

/* Bar chart */
.bar-chart{display:flex;flex-direction:column;gap:10px}
.bar-row{display:flex;align-items:center;gap:12px}
.bar-name{font-size:12px;font-weight:600;color:var(--pri);font-family:'JetBrains Mono','Fira Code',monospace;min-width:80px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.bar-track{flex:1;height:24px;background:var(--bg);border-radius:4px;overflow:hidden;position:relative}
.bar-fill{height:100%;border-radius:4px;transition:width .5s ease;position:relative}
.bar-fill.mem{background:linear-gradient(90deg,var(--pri),var(--pri2))}
.bar-fill.cpu{background:linear-gradient(90deg,var(--blue),#6366f1)}
.bar-fill.pid{background:linear-gradient(90deg,var(--purple),#7c3aed)}
.bar-val{font-size:11px;font-weight:600;color:var(--fg2);min-width:80px;text-align:right;font-family:'JetBrains Mono','Fira Code',monospace}

/* Analytics-specific */
.analytics-toolbar{display:flex;align-items:center;justify-content:space-between;margin-bottom:20px;padding:12px 16px;background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius-lg)}
.range-btns{display:flex;gap:4px}
.range-btn{padding:6px 16px;background:transparent;border:1px solid var(--border);border-radius:var(--radius);color:var(--fg3);cursor:pointer;font-size:12px;font-weight:600;font-family:inherit;transition:all .15s}
.range-btn:hover{color:var(--fg);border-color:#52525b}
.range-btn.active{background:var(--pri);color:#000;border-color:var(--pri)}
.analytics-status{font-size:12px;color:var(--fg3);display:flex;align-items:center;gap:6px}
.analytics-grid{display:grid;grid-template-columns:1fr 1fr;gap:16px;margin-bottom:16px}
.chart-panel{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius-lg);overflow:hidden}
.panel-header{display:flex;align-items:center;justify-content:space-between;padding:14px 20px 0;border-bottom:none}
.panel-title{font-size:13px;font-weight:600;color:var(--fg2);text-transform:uppercase;letter-spacing:.3px}
.panel-cur{font-size:18px;font-weight:700;font-family:'JetBrains Mono','Fira Code',monospace;color:var(--fg)}
.panel-body{padding:8px 12px 12px;height:200px;position:relative}
.panel-body canvas{width:100%!important;height:100%!important}
.panel-body-table{padding:16px 20px}
.metric-row{display:flex;align-items:center;justify-content:space-between;padding:10px 0;border-bottom:1px solid var(--border)}
.metric-row:last-child{border-bottom:none}
.metric-label{font-size:13px;color:var(--fg2)}
.metric-val{font-size:14px;font-weight:600;font-family:'JetBrains Mono','Fira Code',monospace}

/* Actions */
.actions{display:flex;gap:12px;margin-bottom:28px;align-items:center}
.btn{padding:10px 20px;border:1px solid var(--border);border-radius:var(--radius);background:var(--bg2);color:var(--fg);cursor:pointer;font-size:13px;font-weight:500;font-family:inherit;transition:all .15s;display:inline-flex;align-items:center;gap:8px}
.btn:hover{background:var(--bg3);border-color:#52525b}
.btn-pri{background:var(--pri);color:#000;border-color:var(--pri);font-weight:600}
.btn-pri:hover{background:var(--pri2);border-color:var(--pri2)}
.btn-red{color:var(--red);border-color:rgba(239,68,68,.3)}
.btn-red:hover{background:var(--red-bg);border-color:var(--red)}
.btn-sm{padding:6px 14px;font-size:12px}
.spacer{flex:1}
.count{font-size:13px;color:var(--fg3)}

/* Table */
.card{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius-lg);overflow:hidden}
.card+.card{margin-top:20px}
table{width:100%;border-collapse:collapse}
th{text-align:left;padding:12px 20px;font-size:12px;font-weight:500;text-transform:uppercase;letter-spacing:.5px;color:var(--fg3);background:rgba(0,0,0,.15)}
td{padding:14px 20px;font-size:13px;border-top:1px solid rgba(63,63,70,.3)}
tr:hover td{background:var(--pri-bg)}
.empty-state{padding:60px 20px;text-align:center;color:var(--fg3);font-size:14px}
.empty-icon{font-size:48px;margin-bottom:12px;opacity:.4}
.badge{padding:4px 10px;border-radius:20px;font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:.5px}
.b-run{background:var(--green-bg);color:var(--green)}
.b-stop{background:var(--red-bg);color:var(--red)}
.id-cell{font-weight:600;font-family:'JetBrains Mono','Fira Code',monospace;font-size:13px;color:var(--pri)}
.mono{font-family:'JetBrains Mono','Fira Code',monospace;font-size:12px;color:var(--fg2)}

/* Modal */
.overlay{display:none;position:fixed;inset:0;background:rgba(0,0,0,.6);backdrop-filter:blur(4px);z-index:100;align-items:center;justify-content:center}
.overlay.on{display:flex}
.modal{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius-lg);padding:32px;width:520px;max-width:92vw;max-height:90vh;overflow-y:auto;box-shadow:var(--shadow)}
.modal h2{font-size:20px;font-weight:700;margin-bottom:24px}
.form-row{display:flex;gap:16px}
.form-row>.field{flex:1}
.modal-actions{display:flex;gap:12px;justify-content:flex-end;margin-top:28px;padding-top:20px;border-top:1px solid var(--border)}

/* Playground sidebar */
.pg-sidebar{position:fixed;top:0;right:0;width:560px;height:100vh;background:#0c0c0c;border-left:1px solid #2a2a2a;z-index:91;display:none;flex-direction:column;box-shadow:-4px 0 32px rgba(0,0,0,.5)}
.pg-sidebar.open{display:flex}
body.pg-open main{margin-right:560px}
.pg-titlebar{height:40px;background:#1a1a1a;border-bottom:1px solid #2a2a2a;display:flex;align-items:center;padding:0 12px;gap:0;flex-shrink:0}
.pg-tabs{display:flex;gap:0;margin-left:4px;flex:1}
.pg-tab{padding:6px 16px;background:transparent;border:none;border-bottom:2px solid transparent;font-size:12px;font-family:'JetBrains Mono','Fira Code',monospace;color:#71717a;cursor:pointer;display:flex;align-items:center;gap:6px;transition:color .15s,border-color .15s}
.pg-tab:hover{color:#d4d4d4}
.pg-tab.active{color:#eac01b;border-bottom-color:#eac01b}
.pg-tab-icon{font-size:10px}
.pg-close{margin-left:auto;width:28px;height:28px;display:flex;align-items:center;justify-content:center;background:transparent;border:none;color:#71717a;cursor:pointer;border-radius:4px;font-size:16px;transition:all .1s}
.pg-close:hover{background:#333;color:#fff}
.pg-panel{display:none;flex-direction:column;flex:1;overflow:hidden}
.pg-panel.active{display:flex}
/* Terminal panel */
.term-output{flex:1;background:#0c0c0c;padding:16px;font-family:'JetBrains Mono','Fira Code',monospace;font-size:13px;line-height:1.8;white-space:pre-wrap;word-break:break-all;overflow-y:auto;color:#d4d4d4;scrollbar-width:thin;scrollbar-color:#333 transparent}
.term-output::-webkit-scrollbar{width:6px}
.term-output::-webkit-scrollbar-thumb{background:#333;border-radius:3px}
.term-output .so{color:#4ade80}
.term-output .se{color:#f87171}
.term-output .si{color:#555}
.term-output .prompt{color:#eac01b}
.term-input-row{display:flex;align-items:center;border-top:1px solid #2a2a2a;background:#111;padding:0 16px;flex-shrink:0;height:44px}
.term-ps1{color:#eac01b;font-family:'JetBrains Mono','Fira Code',monospace;font-size:13px;margin-right:8px;white-space:nowrap}
.term-input{flex:1;background:transparent;border:none;outline:none;color:#d4d4d4;font-family:'JetBrains Mono','Fira Code',monospace;font-size:13px;caret-color:#eac01b}
.term-input::placeholder{color:#444}
/* Agent panel */
.agent-output{flex:1;background:#0c0c0c;padding:16px;font-family:'JetBrains Mono','Fira Code',monospace;font-size:13px;line-height:1.7;white-space:pre-wrap;word-break:break-all;overflow-y:auto;color:#d4d4d4;scrollbar-width:thin;scrollbar-color:#333 transparent}
.agent-output::-webkit-scrollbar{width:6px}
.agent-output::-webkit-scrollbar-thumb{background:#333;border-radius:3px}
.agent-output .ev-type{color:#eac01b;font-weight:600;font-size:11px}
.agent-output .ev-time{color:#555;font-size:10px;margin-left:8px}
.agent-output .ev-data{color:#a1a1aa;font-size:12px;margin-left:12px;display:block}
.agent-output .ev-text{color:#4ade80}
.agent-output .ev-reason{color:#f97316}
.agent-output .ev-tool{color:#3b82f6}
.agent-output .ev-err{color:#f87171}
.agent-output .ev-status{color:#a78bfa;font-weight:600}
.agent-output .ev-sep{display:block;border-bottom:1px solid #1a1a1a;margin:6px 0}
.agent-input-row{display:flex;align-items:center;border-top:1px solid #2a2a2a;background:#111;padding:0 12px;flex-shrink:0;min-height:44px;gap:6px}
.agent-input{flex:1;background:transparent;border:none;outline:none;color:#d4d4d4;font-family:'JetBrains Mono','Fira Code',monospace;font-size:13px;caret-color:#eac01b;padding:10px 4px}
.agent-input::placeholder{color:#444}
.agent-send{background:var(--pri);color:#000;border:none;border-radius:4px;padding:6px 14px;font-size:12px;font-weight:600;cursor:pointer;font-family:inherit;transition:background .15s}
.agent-send:hover{background:var(--pri2)}
.agent-send:disabled{opacity:.4;cursor:default}
.agent-abort{background:transparent;color:var(--red);border:1px solid rgba(239,68,68,.3);border-radius:4px;padding:6px 10px;font-size:12px;font-weight:600;cursor:pointer;font-family:inherit;display:none}
.agent-abort.on{display:inline-block}
.agent-status-bar{display:flex;align-items:center;gap:8px;padding:4px 12px;background:#111;border-top:1px solid #1a1a1a;font-size:11px;color:#555;flex-shrink:0}
.agent-status-bar .dot{width:6px;height:6px;border-radius:50%;background:#555}
.agent-status-bar .dot.connected{background:#22c55e}
.agent-status-bar .dot.busy{background:#eac01b;animation:pulse 1s infinite}
@keyframes pulse{0%,100%{opacity:1}50%{opacity:.4}}
/* Playground */
@keyframes fadeIn{from{opacity:0;transform:translateY(4px)}to{opacity:1;transform:translateY(0)}}
@media(max-width:1100px){.pg-sidebar{width:100%;border-left:none}body.pg-open main{margin-right:0}}

/* Toast */
.toast{position:fixed;bottom:24px;right:24px;padding:14px 22px;border-radius:var(--radius);font-size:13px;font-weight:500;z-index:200;animation:slideIn .3s ease}
.t-ok{background:var(--green-bg);border:1px solid var(--green);color:var(--green)}
.t-err{background:var(--red-bg);border:1px solid var(--red);color:var(--red)}
@keyframes slideIn{from{transform:translateY(16px);opacity:0}to{transform:translateY(0);opacity:1}}

@media(max-width:900px){.stats{grid-template-columns:repeat(2,1fr)}.charts,.analytics-grid{grid-template-columns:1fr}.panel-body{height:160px}}
@media(max-width:600px){.stats{grid-template-columns:1fr}.form-row{flex-direction:column;gap:0}.actions{flex-wrap:wrap}nav{padding:0 16px}main{padding:16px}td,th{padding:10px 14px}}
</style>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/chartjs-adapter-date-fns@3/dist/chartjs-adapter-date-fns.bundle.min.js"></script>
</head>
<body>

<div id="login-screen">
  <div class="login-box">
    <div class="logo"><svg viewBox="0 0 292.47 61" xmlns="http://www.w3.org/2000/svg"><g transform="translate(37,28)"><polygon points="-24,-17 -24,-28 -13,-22 -2,-28 -2,-17 -13,-11" style="fill:#eac01b;fill-opacity:0.5"/><polygon points="2,-17 2,-28 13,-22 24,-28 24,-17 13,-11" style="fill:#eac01b;fill-opacity:0.5"/><polygon points="-37,6 -37,-6 -26,0 -15,-6 -15,6 -26,12" style="fill:#eac01b"/><polygon points="-11,6 -11,-6 0,0 11,-6 11,6 0,12" style="fill:#eac01b"/><polygon points="15,6 15,-6 26,0 37,-6 37,6 26,12" style="fill:#eac01b"/><polygon points="-24,27 -24,16 -13,22 -2,16 -2,27 -13,33" style="fill:#eac01b;fill-opacity:0.5"/><polygon points="2,27 2,16 13,22 24,16 24,27 13,33" style="fill:#eac01b;fill-opacity:0.5"/></g><path d="M 90.31091,48.035919 V 7.686021 h 6.06603 v 17.087775 h 20.03954 V 7.686021 h 6.06603 v 40.349898 h -6.06603 V 30.054488 H 96.37694 v 17.981431 z m 39.40208,0 V 19.059818 h 5.79522 v 28.976101 z m 2.89761,-33.823505 q -1.5165,0 -2.57264,-1.029058 -1.05614,-1.029058 -1.05614,-2.491403 0,-1.462346 1.05614,-2.491403 1.05614,-1.029058 2.57264,-1.029058 1.54359,0 2.59973,1.029058 1.05614,1.001977 1.05614,2.491403 0,1.489425 -1.05614,2.518483 -1.05614,1.001978 -2.59973,1.001978 z m 18.08976,33.823505 -11.02175,-28.976101 h 6.17434 l 5.76814,16.031637 q 0.8395,2.356001 1.54359,4.712002 0.73117,2.356 1.43526,4.684921 h -1.46234 q 0.70409,-2.328921 1.40818,-4.684921 0.7041,-2.356001 1.51651,-4.712002 l 5.6869,-16.031637 h 6.12018 l -11.02175,28.976101 z m 32.68613,0.649931 q -4.27872,0 -7.39297,-1.922713 -3.08718,-1.922713 -4.73908,-5.307772 -1.65191,-3.412139 -1.65191,-7.826255 0,-4.414117 1.70607,-7.826256 1.70607,-3.439219 4.73908,-5.389013 3.03301,-1.949794 6.98676,-1.949794 2.95177,0 5.38901,1.029058 2.46432,1.029058 4.25164,3.005932 1.78731,1.949794 2.7622,4.739082 1.00198,2.762208 1.00198,6.228508 v 1.706069 h -24.10162 v -4.359955 h 21.09569 l -2.59973,1.354023 q 0,-2.708046 -0.94781,-4.684921 -0.92074,-2.003954 -2.65389,-3.087173 -1.73315,-1.110299 -4.14331,-1.110299 -2.38308,0 -4.14331,1.110299 -1.73315,1.083219 -2.70805,3.060093 -0.94782,1.949794 -0.94782,4.549519 v 2.599725 q 0,2.789288 0.9749,4.874484 0.9749,2.085196 2.78929,3.249656 1.84147,1.13738 4.41412,1.13738 1.84147,0 3.24965,-0.541609 1.40819,-0.56869 2.356,-1.570668 0.9749,-1.029057 1.40819,-2.383081 l 5.52441,0.297885 q -0.56869,2.653886 -2.32892,4.684921 -1.76023,2.031036 -4.41411,3.195496 -2.62681,1.137379 -5.87646,1.137379 z m 18.79384,-0.649931 V 7.686021 h 15.05674 q 4.19747,0 7.09508,1.326943 2.92469,1.326943 4.4412,3.710024 1.54358,2.383081 1.54358,5.551496 0,2.383081 -0.83949,4.116231 -0.8395,1.73315 -2.38308,2.87053 -1.51651,1.13738 -3.57462,1.76023 v 0.108322 q 2.30184,0.297885 4.19747,1.462346 1.89563,1.16446 3.03301,3.222575 1.16446,2.058116 1.16446,4.982807 0,3.330897 -1.57067,5.876461 -1.57066,2.545564 -4.712,3.953749 -3.14133,1.408184 -7.90749,1.408184 z m 6.01186,-5.118208 h 9.09904 q 4.17039,0 6.30975,-1.624829 2.16643,-1.624828 2.16643,-4.576599 0,-2.031035 -0.97489,-3.520461 -0.9749,-1.516506 -2.81637,-2.356001 -1.84147,-0.839494 -4.46828,-0.839494 h -9.31568 z m 0,-17.818949 h 8.85531 q 2.2206,0 3.84543,-0.758253 1.65191,-0.758253 2.51848,-2.139357 0.89366,-1.408184 0.89366,-3.303817 0,-2.762208 -1.89563,-4.414117 -1.86856,-1.678989 -5.33486,-1.678989 h -8.88239 z m 41.64977,23.587088 q -4.11623,0 -7.23049,-1.895632 -3.11425,-1.922714 -4.8474,-5.307772 -1.70607,-3.412139 -1.70607,-7.853336 0,-4.495358 1.70607,-7.907497 1.73315,-3.412139 4.8474,-5.334852 3.11426,-1.922714 7.23049,-1.922714 4.14331,0 7.23049,1.922714 3.11425,1.922713 4.82032,5.334852 1.73315,3.412139 1.73315,7.907497 0,4.441197 -1.73315,7.853336 -1.70607,3.385058 -4.82032,5.307772 -3.08718,1.895632 -7.23049,1.895632 z m 0,-4.955725 q 2.51848,0 4.27871,-1.272782 1.78731,-1.272782 2.70805,-3.547542 0.94782,-2.30184 0.94782,-5.280691 0,-3.033013 -0.94782,-5.307772 -0.92074,-2.30184 -2.70805,-3.574622 -1.76023,-1.299863 -4.27871,-1.299863 -2.4914,0 -4.27871,1.272783 -1.78732,1.272782 -2.73513,3.574621 -0.92074,2.27476 -0.92074,5.334853 0,3.005932 0.92074,5.280691 0.94781,2.27476 2.70805,3.547542 1.78731,1.272782 4.30579,1.272782 z m 15.13798,4.305794 11.83416,-16.708649 -0.0542,3.791266 -11.13007,-16.058718 h 6.55347 l 3.43922,5.19945 q 1.19154,1.841471 2.24768,3.628782 1.08322,1.760231 2.16644,3.520461 h -2.43725 q 1.1103,-1.76023 2.16644,-3.520461 1.05614,-1.787311 2.27476,-3.628782 l 3.49338,-5.19945 h 6.44515 l -11.23839,16.085798 0.0271,-3.764185 11.69876,16.654488 h -6.55347 l -4.00791,-5.930622 q -1.19154,-1.814392 -2.24768,-3.520461 -1.02906,-1.70607 -2.11228,-3.41214 h 2.38309 q -1.08322,1.70607 -2.13936,3.41214 -1.02906,1.706069 -2.2206,3.520461 l -4.06207,5.930622 z" style="fill:#eac01b;font-weight:500;font-size:55.46px" /></svg></div>
    <div class="login-sub">Connect to manage your hiveboxes</div>
    <div class="field">
      <label>API Key</label>
      <input type="password" id="l-key" placeholder="Enter your API key" onkeydown="if(event.key==='Enter')login()">
    </div>
    <button class="btn-primary" onclick="login()">Connect</button>
    <div class="login-err" id="l-err"></div>
  </div>
</div>

<div id="app">
  <nav>
    <div class="nav-brand"><svg viewBox="0 0 292.47 61" xmlns="http://www.w3.org/2000/svg"><g transform="translate(37,28)"><polygon points="-24,-17 -24,-28 -13,-22 -2,-28 -2,-17 -13,-11" style="fill:#eac01b;fill-opacity:0.5"/><polygon points="2,-17 2,-28 13,-22 24,-28 24,-17 13,-11" style="fill:#eac01b;fill-opacity:0.5"/><polygon points="-37,6 -37,-6 -26,0 -15,-6 -15,6 -26,12" style="fill:#eac01b"/><polygon points="-11,6 -11,-6 0,0 11,-6 11,6 0,12" style="fill:#eac01b"/><polygon points="15,6 15,-6 26,0 37,-6 37,6 26,12" style="fill:#eac01b"/><polygon points="-24,27 -24,16 -13,22 -2,16 -2,27 -13,33" style="fill:#eac01b;fill-opacity:0.5"/><polygon points="2,27 2,16 13,22 24,16 24,27 13,33" style="fill:#eac01b;fill-opacity:0.5"/></g><path d="M 90.31091,48.035919 V 7.686021 h 6.06603 v 17.087775 h 20.03954 V 7.686021 h 6.06603 v 40.349898 h -6.06603 V 30.054488 H 96.37694 v 17.981431 z m 39.40208,0 V 19.059818 h 5.79522 v 28.976101 z m 2.89761,-33.823505 q -1.5165,0 -2.57264,-1.029058 -1.05614,-1.029058 -1.05614,-2.491403 0,-1.462346 1.05614,-2.491403 1.05614,-1.029058 2.57264,-1.029058 1.54359,0 2.59973,1.029058 1.05614,1.001977 1.05614,2.491403 0,1.489425 -1.05614,2.518483 -1.05614,1.001978 -2.59973,1.001978 z m 18.08976,33.823505 -11.02175,-28.976101 h 6.17434 l 5.76814,16.031637 q 0.8395,2.356001 1.54359,4.712002 0.73117,2.356 1.43526,4.684921 h -1.46234 q 0.70409,-2.328921 1.40818,-4.684921 0.7041,-2.356001 1.51651,-4.712002 l 5.6869,-16.031637 h 6.12018 l -11.02175,28.976101 z m 32.68613,0.649931 q -4.27872,0 -7.39297,-1.922713 -3.08718,-1.922713 -4.73908,-5.307772 -1.65191,-3.412139 -1.65191,-7.826255 0,-4.414117 1.70607,-7.826256 1.70607,-3.439219 4.73908,-5.389013 3.03301,-1.949794 6.98676,-1.949794 2.95177,0 5.38901,1.029058 2.46432,1.029058 4.25164,3.005932 1.78731,1.949794 2.7622,4.739082 1.00198,2.762208 1.00198,6.228508 v 1.706069 h -24.10162 v -4.359955 h 21.09569 l -2.59973,1.354023 q 0,-2.708046 -0.94781,-4.684921 -0.92074,-2.003954 -2.65389,-3.087173 -1.73315,-1.110299 -4.14331,-1.110299 -2.38308,0 -4.14331,1.110299 -1.73315,1.083219 -2.70805,3.060093 -0.94782,1.949794 -0.94782,4.549519 v 2.599725 q 0,2.789288 0.9749,4.874484 0.9749,2.085196 2.78929,3.249656 1.84147,1.13738 4.41412,1.13738 1.84147,0 3.24965,-0.541609 1.40819,-0.56869 2.356,-1.570668 0.9749,-1.029057 1.40819,-2.383081 l 5.52441,0.297885 q -0.56869,2.653886 -2.32892,4.684921 -1.76023,2.031036 -4.41411,3.195496 -2.62681,1.137379 -5.87646,1.137379 z m 18.79384,-0.649931 V 7.686021 h 15.05674 q 4.19747,0 7.09508,1.326943 2.92469,1.326943 4.4412,3.710024 1.54358,2.383081 1.54358,5.551496 0,2.383081 -0.83949,4.116231 -0.8395,1.73315 -2.38308,2.87053 -1.51651,1.13738 -3.57462,1.76023 v 0.108322 q 2.30184,0.297885 4.19747,1.462346 1.89563,1.16446 3.03301,3.222575 1.16446,2.058116 1.16446,4.982807 0,3.330897 -1.57067,5.876461 -1.57066,2.545564 -4.712,3.953749 -3.14133,1.408184 -7.90749,1.408184 z m 6.01186,-5.118208 h 9.09904 q 4.17039,0 6.30975,-1.624829 2.16643,-1.624828 2.16643,-4.576599 0,-2.031035 -0.97489,-3.520461 -0.9749,-1.516506 -2.81637,-2.356001 -1.84147,-0.839494 -4.46828,-0.839494 h -9.31568 z m 0,-17.818949 h 8.85531 q 2.2206,0 3.84543,-0.758253 1.65191,-0.758253 2.51848,-2.139357 0.89366,-1.408184 0.89366,-3.303817 0,-2.762208 -1.89563,-4.414117 -1.86856,-1.678989 -5.33486,-1.678989 h -8.88239 z m 41.64977,23.587088 q -4.11623,0 -7.23049,-1.895632 -3.11425,-1.922714 -4.8474,-5.307772 -1.70607,-3.412139 -1.70607,-7.853336 0,-4.495358 1.70607,-7.907497 1.73315,-3.412139 4.8474,-5.334852 3.11426,-1.922714 7.23049,-1.922714 4.14331,0 7.23049,1.922714 3.11425,1.922713 4.82032,5.334852 1.73315,3.412139 1.73315,7.907497 0,4.441197 -1.73315,7.853336 -1.70607,3.385058 -4.82032,5.307772 -3.08718,1.895632 -7.23049,1.895632 z m 0,-4.955725 q 2.51848,0 4.27871,-1.272782 1.78731,-1.272782 2.70805,-3.547542 0.94782,-2.30184 0.94782,-5.280691 0,-3.033013 -0.94782,-5.307772 -0.92074,-2.30184 -2.70805,-3.574622 -1.76023,-1.299863 -4.27871,-1.299863 -2.4914,0 -4.27871,1.272783 -1.78732,1.272782 -2.73513,3.574621 -0.92074,2.27476 -0.92074,5.334853 0,3.005932 0.92074,5.280691 0.94781,2.27476 2.70805,3.547542 1.78731,1.272782 4.30579,1.272782 z m 15.13798,4.305794 11.83416,-16.708649 -0.0542,3.791266 -11.13007,-16.058718 h 6.55347 l 3.43922,5.19945 q 1.19154,1.841471 2.24768,3.628782 1.08322,1.760231 2.16644,3.520461 h -2.43725 q 1.1103,-1.76023 2.16644,-3.520461 1.05614,-1.787311 2.27476,-3.628782 l 3.49338,-5.19945 h 6.44515 l -11.23839,16.085798 0.0271,-3.764185 11.69876,16.654488 h -6.55347 l -4.00791,-5.930622 q -1.19154,-1.814392 -2.24768,-3.520461 -1.02906,-1.70607 -2.11228,-3.41214 h 2.38309 q -1.08322,1.70607 -2.13936,3.41214 -1.02906,1.706069 -2.2206,3.520461 l -4.06207,5.930622 z" style="fill:#eac01b;font-weight:500;font-size:55.46px" /></svg></div>
    <div class="nav-right">
      <span class="nav-status"><span class="status-dot"></span>Connected</span>
      <button class="btn-icon" id="theme-toggle" onclick="toggleTheme()" title="Toggle theme">&#9790;</button>
      <button class="btn-ghost" onclick="logout()">Logout</button>
    </div>
  </nav>

  <main>
    <!-- Stats overview -->
    <div class="stats">
      <div class="stat-card">
        <div class="stat-label">Active HiveBoxes</div>
        <div class="stat-value" id="st-active">0</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">Total Memory Used</div>
        <div class="stat-value" id="st-mem">0 <span class="unit">MB</span></div>
      </div>
      <div class="stat-card">
        <div class="stat-label">Total CPUs</div>
        <div class="stat-value" id="st-cpu">0</div>
      </div>
      <div class="stat-card">
        <div class="stat-label">Commands Executed</div>
        <div class="stat-value" id="st-cmds">0</div>
      </div>
    </div>

    <!-- Tabs -->
    <div class="tabs">
      <button class="tab active" onclick="switchTab('hiveboxes')">HiveBoxes</button>
      <button class="tab" onclick="switchTab('analytics')">Analytics</button>
    </div>

    <!-- Tab: HiveBoxes -->
    <div class="tab-content active" id="tab-hiveboxes">
      <div class="actions">
        <button class="btn btn-pri" onclick="openCreate()">+ New HiveBox</button>
        <button class="btn" onclick="refresh()">Refresh</button>
        <span class="spacer"></span>
        <span class="count" id="s-cnt"></span>
      </div>

      <div class="card">
        <div id="s-tbl"></div>
      </div>

    </div>

    <!-- Tab: Analytics -->
    <div class="tab-content" id="tab-analytics">
      <!-- Time range selector -->
      <div class="analytics-toolbar">
        <div class="range-btns">
          <button class="range-btn" onclick="setRange(300)" data-range="300">5m</button>
          <button class="range-btn active" onclick="setRange(900)" data-range="900">15m</button>
          <button class="range-btn" onclick="setRange(3600)" data-range="3600">1h</button>
          <button class="range-btn" onclick="setRange(21600)" data-range="21600">6h</button>
        </div>
        <span class="analytics-status"><span class="status-dot"></span>Auto-refresh 5s</span>
      </div>

      <!-- Chart panels row 1 -->
      <div class="analytics-grid">
        <div class="chart-panel">
          <div class="panel-header"><span class="panel-title">Memory Usage</span><span id="a-mem-cur" class="panel-cur"></span></div>
          <div class="panel-body"><canvas id="chart-mem"></canvas></div>
        </div>
        <div class="chart-panel">
          <div class="panel-header"><span class="panel-title">CPU Usage</span><span id="a-cpu-cur" class="panel-cur"></span></div>
          <div class="panel-body"><canvas id="chart-cpu"></canvas></div>
        </div>
      </div>

      <!-- Chart panels row 2 -->
      <div class="analytics-grid">
        <div class="chart-panel">
          <div class="panel-header"><span class="panel-title">Processes</span><span id="a-pid-cur" class="panel-cur"></span></div>
          <div class="panel-body"><canvas id="chart-pids"></canvas></div>
        </div>
        <div class="chart-panel">
          <div class="panel-header"><span class="panel-title">Sandboxes</span><span id="a-cnt-cur" class="panel-cur"></span></div>
          <div class="panel-body"><canvas id="chart-count"></canvas></div>
        </div>
      </div>

      <!-- Per-sandbox breakdown -->
      <div class="analytics-grid">
        <div class="chart-panel">
          <div class="panel-header"><span class="panel-title">Per-HiveBox Memory</span></div>
          <div class="panel-body"><canvas id="chart-per-mem"></canvas></div>
        </div>
        <div class="chart-panel">
          <div class="panel-header"><span class="panel-title">Per-HiveBox CPU</span></div>
          <div class="panel-body"><canvas id="chart-per-cpu"></canvas></div>
        </div>
      </div>

      <!-- Summary + details -->
      <div class="analytics-grid">
        <div class="chart-panel">
          <div class="panel-header"><span class="panel-title">Resource Summary</span></div>
          <div class="panel-body-table" id="a-summary"></div>
        </div>
        <div class="chart-panel">
          <div class="panel-header"><span class="panel-title">Per-HiveBox Details</span></div>
          <div class="panel-body-table" id="a-details" style="max-height:300px;overflow-y:auto"></div>
        </div>
      </div>

      <div id="analytics-empty" class="card" style="display:none">
        <div class="empty-state"><div class="empty-icon">&#x1F4CA;</div>No hiveboxes running.<br>Create one to see analytics.</div>
      </div>
    </div>
  </main>
</div>

<!-- Terminal sidebar -->
<div class="pg-sidebar" id="pg-sidebar">
  <div class="pg-titlebar">
    <div class="pg-tabs">
      <button class="pg-tab active" id="pg-tab-term" onclick="pgSwitchTab('term')"><span class="pg-tab-icon">&#9658;</span> Terminal</button>
      <button class="pg-tab" id="pg-tab-agent" onclick="pgSwitchTab('agent')"><span class="pg-tab-icon">&#9679;</span> Agent</button>
    </div>
    <span id="pg-sandbox-id" style="color:#71717a;font-size:11px;font-family:'JetBrains Mono','Fira Code',monospace;margin-right:8px"></span>
    <button class="pg-close" onclick="closePlayground()">&times;</button>
  </div>
  <!-- Terminal panel -->
  <div class="pg-panel active" id="pg-panel-term">
    <div class="term-output" id="ex-out"><span class="si">~ Welcome to HiveBox terminal</span>
<span class="si">~ Type a command and press Enter</span>
</div>
    <div class="term-input-row">
      <span class="term-ps1" id="term-ps1">$</span>
      <input type="text" class="term-input" id="ex-cmd" placeholder="type command..." onkeydown="termKeydown(event)">
    </div>
  </div>
  <!-- Agent panel -->
  <div class="pg-panel" id="pg-panel-agent">
    <div class="agent-output" id="ag-out"><span class="si">~ Agent playground</span>
<span class="si">~ Send a prompt to interact with the AI agent</span>
</div>
    <div class="agent-input-row">
      <input type="text" class="agent-input" id="ag-input" placeholder="send a prompt..." onkeydown="if(event.key==='Enter')agentSend()">
      <button class="agent-abort" id="ag-abort" onclick="agentAbort()">Stop</button>
      <button class="agent-send" id="ag-send" onclick="agentSend()">Send</button>
    </div>
    <div class="agent-status-bar">
      <span class="dot" id="ag-dot"></span>
      <span id="ag-status">Disconnected</span>
      <span style="margin-left:auto" id="ag-session"></span>
    </div>
  </div>
</div>

<!-- Create modal -->
<div class="overlay" id="c-modal">
  <div class="modal">
    <h2>Create HiveBox</h2>
    <div class="field">
      <label>Name</label>
      <input type="text" id="c-name" placeholder="Optional &mdash; auto-generated if empty">
    </div>
    <div class="field">
      <label>Network</label>
      <select id="c-net"><option value="none">None (no network)</option><option value="isolated">Isolated (NAT to internet)</option></select>
    </div>
    <div class="form-row">
      <div class="field"><label>Memory</label><input type="text" id="c-mem" value="256m"></div>
      <div class="field"><label>CPUs</label><input type="number" id="c-cpu" value="1.0" step="0.1" min="0.1"></div>
      <div class="field"><label>Max PIDs</label><input type="number" id="c-pid" value="64" min="1"></div>
    </div>
    <div class="field">
      <label>Timeout (seconds)</label>
      <input type="number" id="c-tout" value="3600" min="60" max="86400">
    </div>
    <details style="margin-top:12px">
      <summary style="cursor:pointer;color:#888;font-size:13px;user-select:none">Advanced (LLM, Skills, MCPs)</summary>
      <div style="margin-top:8px">
        <div class="form-row">
          <div class="field" style="flex:2"><label>LLM Base URL</label><input type="text" id="c-llm-url" placeholder="Global default"></div>
          <div class="field" style="flex:1"><label>Model</label><input type="text" id="c-llm-model" placeholder="Global default"></div>
        </div>
        <div class="field">
          <label>LLM API Key</label>
          <input type="password" id="c-llm-key" placeholder="Global default">
        </div>
        <div class="field">
          <label>Skills</label>
          <input type="text" id="c-skills" placeholder="All defaults. Comma-separated names or &quot;none&quot;">
        </div>
        <div class="field">
          <label>Custom MCPs (JSON)</label>
          <textarea id="c-mcps" rows="3" style="width:100%;font-family:monospace;font-size:12px;background:#1e1e1e;color:#d4d4d4;border:1px solid #333;border-radius:4px;padding:6px" placeholder='{"my-server":{"type":"remote","url":"...","enabled":true}}'></textarea>
        </div>
      </div>
    </details>
    <div class="modal-actions">
      <button class="btn" onclick="closeCreate()">Cancel</button>
      <button class="btn btn-pri" onclick="doCreate()">Create</button>
    </div>
  </div>
</div>

<script>
let KEY='',CUR=null,SANDBOXES=[];
let ANALYTICS_RANGE=900; // default 15 min
let CHARTS={};
let ANALYTICS_DATA=null;

// Tabs
function switchTab(name){
  document.querySelectorAll('.tab').forEach(t=>t.classList.remove('active'));
  document.querySelectorAll('.tab-content').forEach(t=>t.classList.remove('active'));
  document.querySelector(`.tab-content#tab-${name}`).classList.add('active');
  event.target.classList.add('active');
}

// Theme
function toggleTheme(){
  const h=document.documentElement;
  const light=h.classList.toggle('light');
  localStorage.setItem('hb_theme',light?'light':'dark');
  document.getElementById('theme-toggle').innerHTML=light?'&#9728;':'&#9790;';
}
(function(){
  if(localStorage.getItem('hb_theme')==='light'){
    document.documentElement.classList.add('light');
    document.getElementById('theme-toggle').innerHTML='&#9728;';
  }
})();

// Auth
async function login(){
  KEY=document.getElementById('l-key').value;
  const e=document.getElementById('l-err');
  try{
    const r=await fetch('/api/v1/hiveboxes',{headers:{'Authorization':'Bearer '+KEY}});
    if(r.status===401){e.textContent='Invalid API key';e.style.display='block';return}
    document.getElementById('login-screen').style.display='none';
    document.getElementById('app').style.display='block';
    localStorage.setItem('hb_k',KEY);
    refresh();
  }catch(x){e.textContent='Connection failed: '+x.message;e.style.display='block'}
}
function logout(){localStorage.removeItem('hb_k');location.reload()}
(function(){const k=localStorage.getItem('hb_k');if(k){document.getElementById('l-key').value=k;login()}})();

// API helper
async function api(m,p,b){
  const o={method:m,headers:{'Authorization':'Bearer '+KEY,'Content-Type':'application/json'}};
  if(b)o.body=JSON.stringify(b);
  const r=await fetch(p,o);
  const t=await r.text();
  let d;try{d=JSON.parse(t)}catch{d=t}
  return{s:r.status,d}
}

function toast(m,ok=true){
  const e=document.createElement('div');
  e.className='toast '+(ok?'t-ok':'t-err');
  e.textContent=m;document.body.appendChild(e);
  setTimeout(()=>e.remove(),3000);
}
function esc(s){if(!s)return'';const d=document.createElement('div');d.textContent=String(s);return d.innerHTML}
function dur(s){if(s==null)return'-';if(s<60)return s+'s';if(s<3600)return Math.floor(s/60)+'m '+Math.floor(s%60)+'s';return Math.floor(s/3600)+'h '+Math.floor((s%3600)/60)+'m'}
function parseMem(s){if(!s)return 0;s=s.toLowerCase();if(s.endsWith('g'))return parseFloat(s)*1024;if(s.endsWith('m'))return parseFloat(s);if(s.endsWith('k'))return parseFloat(s)/1024;return parseFloat(s)/(1024*1024)}
function fmtBytes(b){if(!b)return'0 MB';const mb=b/(1024*1024);if(mb<1)return(b/1024).toFixed(0)+' KB';if(mb>=1024)return(mb/1024).toFixed(1)+' GB';return mb.toFixed(1)+' MB'}

// Stats
async function updateStats(sandboxes){
  SANDBOXES=sandboxes;
  const running=sandboxes.filter(s=>s.status==='running');
  document.getElementById('st-active').textContent=running.length;
  let totalMemUsed=0,totalCpu=0,totalCmds=0;
  for(const s of sandboxes){
    totalMemUsed+=s.memory_usage_bytes||0;
    totalCpu+=s.cpus||0;
    totalCmds+=s.commands_executed||0;
  }
  document.getElementById('st-mem').innerHTML=fmtBytes(totalMemUsed)+' <span class="unit">used</span>';
  document.getElementById('st-cpu').textContent=totalCpu.toFixed(1);
  document.getElementById('st-cmds').textContent=totalCmds;

  // Fetch analytics from server
  try{
    const ar=await api('GET','/api/v1/analytics?range='+ANALYTICS_RANGE);
    if(ar.s===200) ANALYTICS_DATA=ar.d;
  }catch(e){}

  updateAnalytics(sandboxes);
}

// Time range selector
function setRange(secs){
  ANALYTICS_RANGE=secs;
  document.querySelectorAll('.range-btn').forEach(b=>b.classList.toggle('active',parseInt(b.dataset.range)===secs));
  if(SANDBOXES.length) refresh();
}

// Chart.js defaults
function initChartDefaults(){
  if(!window.Chart) return;
  const isDark=!document.documentElement.classList.contains('light');
  const gridColor=isDark?'rgba(63,63,70,.5)':'rgba(222,226,230,.7)';
  const textColor=isDark?'#a1a1aa':'#6b7280';
  Chart.defaults.color=textColor;
  Chart.defaults.borderColor=gridColor;
  Chart.defaults.font.family="'JetBrains Mono','Fira Code',monospace";
  Chart.defaults.font.size=11;
  Chart.defaults.animation.duration=300;
  Chart.defaults.plugins.legend.display=false;
  Chart.defaults.plugins.tooltip.backgroundColor=isDark?'#27272a':'#ffffff';
  Chart.defaults.plugins.tooltip.titleColor=isDark?'#fafafa':'#1a1a1a';
  Chart.defaults.plugins.tooltip.bodyColor=isDark?'#a1a1aa':'#6b7280';
  Chart.defaults.plugins.tooltip.borderColor=isDark?'#3f3f46':'#dee2e6';
  Chart.defaults.plugins.tooltip.borderWidth=1;
  Chart.defaults.plugins.tooltip.cornerRadius=6;
  Chart.defaults.plugins.tooltip.padding=10;
  Chart.defaults.plugins.tooltip.displayColors=true;
  Chart.defaults.plugins.tooltip.mode='index';
  Chart.defaults.plugins.tooltip.intersect=false;
}

function makeTimeChart(canvasId,label,color,unitCb){
  const ctx=document.getElementById(canvasId);
  if(!ctx||!window.Chart) return null;
  const gradient=ctx.getContext('2d');
  const isDark=!document.documentElement.classList.contains('light');
  return new Chart(ctx,{
    type:'line',
    data:{labels:[],datasets:[{label,data:[],borderColor:color,backgroundColor:color+'18',borderWidth:2,fill:true,tension:.3,pointRadius:0,pointHitRadius:8}]},
    options:{
      responsive:true,maintainAspectRatio:false,
      interaction:{mode:'index',intersect:false},
      scales:{
        x:{type:'time',time:{tooltipFormat:'HH:mm:ss',displayFormats:{second:'HH:mm:ss',minute:'HH:mm',hour:'HH:mm'}},grid:{display:false},ticks:{maxTicksLimit:8,maxRotation:0}},
        y:{beginAtZero:true,grid:{color:isDark?'rgba(63,63,70,.3)':'rgba(222,226,230,.5)'},ticks:{maxTicksLimit:6,callback:unitCb||function(v){return v}}}
      },
      plugins:{tooltip:{callbacks:{label:function(ctx){return ctx.dataset.label+': '+(unitCb?unitCb(ctx.parsed.y):ctx.parsed.y)}}}}
    }
  });
}

function makeMultiChart(canvasId,unitCb){
  const ctx=document.getElementById(canvasId);
  if(!ctx||!window.Chart) return null;
  const isDark=!document.documentElement.classList.contains('light');
  return new Chart(ctx,{
    type:'line',
    data:{labels:[],datasets:[]},
    options:{
      responsive:true,maintainAspectRatio:false,
      interaction:{mode:'index',intersect:false},
      scales:{
        x:{type:'time',time:{tooltipFormat:'HH:mm:ss',displayFormats:{second:'HH:mm:ss',minute:'HH:mm',hour:'HH:mm'}},grid:{display:false},ticks:{maxTicksLimit:8,maxRotation:0}},
        y:{beginAtZero:true,grid:{color:isDark?'rgba(63,63,70,.3)':'rgba(222,226,230,.5)'},ticks:{maxTicksLimit:6,callback:unitCb||function(v){return v}}}
      },
      plugins:{legend:{display:true,position:'bottom',labels:{boxWidth:12,padding:8,font:{size:10}}},tooltip:{callbacks:{label:function(ctx){return ctx.dataset.label+': '+(unitCb?unitCb(ctx.parsed.y):ctx.parsed.y)}}}}
    }
  });
}

const SANDBOX_COLORS=['#eac01b','#3b82f6','#22c55e','#ef4444','#a78bfa','#f97316','#06b6d4','#ec4899','#84cc16','#6366f1'];

function fmtMB(v){if(v==null)return'';const mb=v/(1024*1024);if(mb>=1024)return(mb/1024).toFixed(1)+' GB';if(mb>=1)return mb.toFixed(1)+' MB';return(v/1024).toFixed(0)+' KB'}
function fmtCpuPct(v){if(v==null)return'';if(v>0&&v<0.1)return v.toFixed(3)+'%';if(v<1)return v.toFixed(2)+'%';return v.toFixed(1)+'%'}

function initCharts(){
  if(!window.Chart) return;
  initChartDefaults();
  // Destroy existing
  Object.values(CHARTS).forEach(c=>{if(c)c.destroy()});
  CHARTS.mem=makeTimeChart('chart-mem','Memory',getComputedStyle(document.documentElement).getPropertyValue('--pri').trim()||'#eac01b',fmtMB);
  CHARTS.cpu=makeTimeChart('chart-cpu','CPU %','#3b82f6',fmtCpuPct);
  CHARTS.pids=makeTimeChart('chart-pids','Processes','#a78bfa',function(v){return Math.round(v)});
  CHARTS.count=makeTimeChart('chart-count','Sandboxes','#22c55e',function(v){return Math.round(v)});
  CHARTS.perMem=makeMultiChart('chart-per-mem',fmtMB);
  CHARTS.perCpu=makeMultiChart('chart-per-cpu',fmtCpuPct);
}

// Analytics tab update
function updateAnalytics(sandboxes){
  const empty=document.getElementById('analytics-empty');

  if(!ANALYTICS_DATA||!ANALYTICS_DATA.samples||!ANALYTICS_DATA.samples.length){
    empty.style.display='block';return;
  }
  empty.style.display='none';

  if(!CHARTS.mem) initCharts();
  if(!CHARTS.mem) return; // Chart.js not loaded

  const samples=ANALYTICS_DATA.samples;
  const interval=ANALYTICS_DATA.interval_secs||5;
  const labels=samples.map(s=>new Date(s.timestamp*1000));

  // Memory chart — host/container level from /proc/meminfo
  const memData=samples.map(s=>s.host_memory_used||0);
  const memTotal=samples.length?(samples[samples.length-1].host_memory_total||0):0;
  CHARTS.mem.data.labels=labels;
  CHARTS.mem.data.datasets[0].data=memData;
  CHARTS.mem.update('none');
  const lastMem=memData[memData.length-1]||0;
  document.getElementById('a-mem-cur').textContent=fmtMB(lastMem)+(memTotal?' / '+fmtMB(memTotal):'');

  // CPU % chart — host/container level from /proc/stat
  const cpuData=samples.map(s=>s.host_cpu_percent||0);
  CHARTS.cpu.data.labels=labels;
  CHARTS.cpu.data.datasets[0].data=cpuData;
  CHARTS.cpu.update('none');
  const lastCpu=cpuData[cpuData.length-1]||0;
  document.getElementById('a-cpu-cur').textContent=lastCpu.toFixed(1)+'%';

  // PIDs chart
  const pidData=samples.map(s=>s.total_pids);
  CHARTS.pids.data.labels=labels;
  CHARTS.pids.data.datasets[0].data=pidData;
  CHARTS.pids.update('none');
  document.getElementById('a-pid-cur').textContent=pidData[pidData.length-1]||0;

  // Count chart
  const cntData=samples.map(s=>s.sandbox_count);
  CHARTS.count.data.labels=labels;
  CHARTS.count.data.datasets[0].data=cntData;
  CHARTS.count.update('none');
  document.getElementById('a-cnt-cur').textContent=cntData[cntData.length-1]||0;

  // Per-sandbox memory (stacked area)
  const sbIds=new Set();
  samples.forEach(s=>(s.sandboxes||[]).forEach(sb=>sbIds.add(sb.id)));
  const ids=[...sbIds];
  CHARTS.perMem.data.labels=labels;
  CHARTS.perMem.data.datasets=ids.map((id,i)=>({
    label:id.substring(0,12),data:samples.map(s=>{const sb=(s.sandboxes||[]).find(x=>x.id===id);return sb?sb.memory_bytes:0}),
    borderColor:SANDBOX_COLORS[i%SANDBOX_COLORS.length],backgroundColor:SANDBOX_COLORS[i%SANDBOX_COLORS.length]+'30',
    borderWidth:1.5,fill:false,tension:.3,pointRadius:0
  }));
  CHARTS.perMem.update('none');

  // Per-sandbox CPU % (stacked area)
  CHARTS.perCpu.data.labels=labels;
  CHARTS.perCpu.data.datasets=ids.map((id,i)=>({
    label:id.substring(0,12),data:samples.map((s,j)=>{
      if(j===0)return 0;
      const cur=(s.sandboxes||[]).find(x=>x.id===id);
      const prev=(samples[j-1].sandboxes||[]).find(x=>x.id===id);
      if(!cur||!prev)return 0;
      const delta=Math.max(0,cur.cpu_usec-prev.cpu_usec);
      return delta/(interval*1e6)*100;
    }),
    borderColor:SANDBOX_COLORS[i%SANDBOX_COLORS.length],backgroundColor:SANDBOX_COLORS[i%SANDBOX_COLORS.length]+'30',
    borderWidth:1.5,fill:false,tension:.3,pointRadius:0
  }));
  CHARTS.perCpu.update('none');

  // Summary
  const summary=document.getElementById('a-summary');
  const totalMem=sandboxes.reduce((a,s)=>a+(s.memory_usage_bytes||0),0);
  const totalMemLimit=sandboxes.reduce((a,s)=>a+parseMem(s.memory)*1024*1024,0);
  const totalPids=sandboxes.reduce((a,s)=>a+(s.pid_current||0),0);
  const totalCpuSec=sandboxes.reduce((a,s)=>a+(s.cpu_usage_usec||0),0)/1e6;
  const avgUptime=sandboxes.length?sandboxes.reduce((a,s)=>a+(s.uptime_seconds||0),0)/sandboxes.length:0;
  summary.innerHTML=`
    <div class="metric-row"><span class="metric-label">Host Memory</span><span class="metric-val">${fmtMB(lastMem)} / ${fmtMB(memTotal)}</span></div>
    <div class="metric-row"><span class="metric-label">Host CPU</span><span class="metric-val">${lastCpu.toFixed(1)}%</span></div>
    <div class="metric-row"><span class="metric-label">Sandbox Memory Used / Allocated</span><span class="metric-val">${fmtBytes(totalMem)} / ${fmtBytes(totalMemLimit)}</span></div>
    <div class="metric-row"><span class="metric-label">Sandbox CPU Time</span><span class="metric-val">${totalCpuSec.toFixed(1)}s</span></div>
    <div class="metric-row"><span class="metric-label">Total Processes</span><span class="metric-val">${totalPids}</span></div>
    <div class="metric-row"><span class="metric-label">Avg Uptime</span><span class="metric-val">${dur(Math.round(avgUptime))}</span></div>
    <div class="metric-row"><span class="metric-label">Total Commands</span><span class="metric-val">${sandboxes.reduce((a,s)=>a+(s.commands_executed||0),0)}</span></div>
  `;

  // Per-hivebox details table
  const details=document.getElementById('a-details');
  let dh='<table><tr><th>Name</th><th>Memory</th><th>CPU %</th><th>CPU Time</th><th>PIDs</th><th>Uptime</th></tr>';
  for(const s of sandboxes){
    const memB=s.memory_usage_bytes||0;
    const memStr=fmtMB(memB);
    const cpuS=((s.cpu_usage_usec||0)/1e6).toFixed(2);
    const cpuPctRaw=s.uptime_seconds>0?((s.cpu_usage_usec||0)/1e6/s.uptime_seconds*100):0;
    const cpuPct=fmtCpuPct(cpuPctRaw);
    dh+=`<tr><td class="id-cell">${esc(s.id).substring(0,12)}</td><td class="mono">${memStr}</td><td class="mono">${cpuPct}</td><td class="mono">${cpuS}s</td><td class="mono">${s.pid_current||0}</td><td class="mono">${dur(s.uptime_seconds)}</td></tr>`;
  }
  details.innerHTML=dh+'</table>';
}

// Re-init charts on theme toggle
const _origToggle=toggleTheme;
toggleTheme=function(){_origToggle();if(CHARTS.mem){initCharts();if(ANALYTICS_DATA)updateAnalytics(SANDBOXES);}}

// Main refresh
async function refresh(){
  const{s,d}=await api('GET','/api/v1/hiveboxes');
  const c=document.getElementById('s-tbl'),n=document.getElementById('s-cnt');
  if(s!==200||!d.sandboxes){c.innerHTML='<div class="empty-state">Failed to load</div>';return}
  n.textContent=d.total+' hivebox'+(d.total!==1?'es':'');
  await updateStats(d.sandboxes);
  if(!d.sandboxes.length){
    c.innerHTML='<div class="empty-state"><div class="empty-icon">&#x2B21;</div>No hiveboxes running.<br>Create one to get started.</div>';
    return;
  }
  let h='<table><tr><th>Name</th><th>Status</th><th>Uptime</th><th>TTL</th><th>Cmds</th><th>Network</th><th>Memory</th><th>CPU</th><th>Procs</th><th></th></tr>';
  for(const x of d.sandboxes){
    const bc=x.status==='running'?'b-run':'b-stop';
    const net=x.network==='none'?'<span style="color:var(--fg3)">none</span>':'<span style="color:var(--blue)">'+esc(x.network)+'</span>';
    const memUsed=fmtBytes(x.memory_usage_bytes);
    const cpuPct=x.uptime_seconds>0?((x.cpu_usage_usec||0)/1e6/x.uptime_seconds*100).toFixed(1):'0.0';
    const cpuSec=((x.cpu_usage_usec||0)/1e6).toFixed(1);
    h+=`<tr>
      <td class="id-cell">${esc(x.id)}</td>
      <td><span class="badge ${bc}">${esc(x.status)}</span></td>
      <td class="mono">${dur(x.uptime_seconds)}</td>
      <td class="mono">${dur(x.ttl_seconds)}</td>
      <td class="mono">${x.commands_executed}</td>
      <td>${net}</td>
      <td class="mono">${memUsed} / ${esc(x.memory)}</td>
      <td class="mono">${cpuPct}% <span style="color:var(--fg3)">(${cpuSec}s)</span></td>
      <td class="mono">${x.pid_current||0}</td>
      <td style="text-align:right">
        <button class="btn btn-sm" style="color:var(--pri);border-color:rgba(234,192,27,.3)" onclick="openPlayground('${esc(x.id)}')">Terminal</button>
        <button class="btn btn-sm" style="color:var(--blue);border-color:rgba(59,130,246,.3)" onclick="openPlayground('${esc(x.id)}','agent')">Agent</button>
        <button class="btn btn-sm btn-red" onclick="destroy('${esc(x.id)}')">Destroy</button>
      </td></tr>`;
  }
  c.innerHTML=h+'</table>';
}

// Create
function openCreate(){document.getElementById('c-modal').classList.add('on');document.getElementById('c-name').focus()}
function closeCreate(){document.getElementById('c-modal').classList.remove('on')}
async function doCreate(){
  const b={
    memory:document.getElementById('c-mem').value,
    cpus:parseFloat(document.getElementById('c-cpu').value),
    pids:parseInt(document.getElementById('c-pid').value),
    timeout:parseInt(document.getElementById('c-tout').value),
  };
  const nm=document.getElementById('c-name').value.trim();
  if(nm)b.name=nm;
  const nv=document.getElementById('c-net').value;
  if(nv!=='none')b.network=nv;
  // LLM config
  const llmUrl=document.getElementById('c-llm-url').value.trim();
  const llmKey=document.getElementById('c-llm-key').value.trim();
  const llmModel=document.getElementById('c-llm-model').value.trim();
  if(llmUrl)b.llm_base_url=llmUrl;
  if(llmKey)b.llm_api_key=llmKey;
  if(llmModel)b.llm_model=llmModel;
  // Skills
  const sk=document.getElementById('c-skills').value.trim();
  if(sk==='none')b.skills=[];
  else if(sk)b.skills=sk.split(',').map(s=>s.trim()).filter(Boolean);
  // Custom MCPs
  const mcpRaw=document.getElementById('c-mcps').value.trim();
  if(mcpRaw){try{b.custom_mcps=JSON.parse(mcpRaw)}catch(e){toast('Invalid MCP JSON: '+e.message,false);return}}
  const{s,d}=await api('POST','/api/v1/hiveboxes',b);
  if(s===200||s===201){toast('HiveBox created: '+(d.id||'ok'));closeCreate();refresh()}
  else toast(d.error||'Error '+s,false);
}

// Destroy
async function destroy(id){
  if(!confirm('Destroy hivebox "'+id+'"?'))return;
  const{s,d}=await api('DELETE','/api/v1/hiveboxes/'+id);
  if(s===200){toast('Destroyed: '+id);if(PG_CUR===id)closePlayground();refresh()}
  else toast(d.error||'Error '+s,false);
}

// ── Terminal sidebar ────────
let CWD='/';
let PG_CUR=null; // current sandbox id for terminal
let PG_TAB='term'; // active tab
let PG_TERM_INITED={}; // track if terminal was initialized per sandbox
let CMD_HISTORY=[]; // command history
let CMD_HIST_IDX=-1; // current position in history (-1 = not browsing)
let CMD_SAVED=''; // saved current input when browsing history

function shortCwd(){
  if(CWD==='/')return '~';
  if(CWD.startsWith('/'))return '~/'+CWD.slice(1);
  return CWD;
}
function updatePs1(){document.getElementById('term-ps1').textContent=shortCwd()+' $';}

function openPlayground(id,tab){
  const isNewSandbox=(PG_CUR!==id);
  PG_CUR=id;CUR=id;
  document.getElementById('pg-sidebar').classList.add('open');
  document.body.classList.add('pg-open');
  document.getElementById('pg-sandbox-id').textContent=id;
  if(isNewSandbox){
    CWD='/';updatePs1();
    document.getElementById('ex-out').innerHTML='<span class="si">~ Connected to <span class="prompt">'+esc(id)+'</span></span>\n<span class="si">~ Type a command and press Enter</span>\n';
    document.getElementById('ex-cmd').value='';
    PG_TERM_INITED[id]=true;
    // Reconnect agent for new sandbox
    if(AG_EVT){AG_EVT.close();AG_EVT=null;}
    AG_SES=null;AG_BOX=null;
    document.getElementById('ag-dot').className='dot';
    document.getElementById('ag-status').textContent='Disconnected';
    document.getElementById('ag-session').textContent='';
    document.getElementById('ag-out').innerHTML='<span class="si">~ Agent playground</span>\n<span class="si">~ Switch to Agent tab to connect</span>\n';
  }
  pgSwitchTab(tab||'term');
}

function closePlayground(){
  PG_CUR=null;CUR=null;
  document.getElementById('pg-sidebar').classList.remove('open');
  document.body.classList.remove('pg-open');
  if(AG_EVT){AG_EVT.close();AG_EVT=null;}
  AG_SES=null;AG_BUSY=false;AG_BOX=null;
}

function pgSwitchTab(tab){
  PG_TAB=tab;
  document.querySelectorAll('.pg-tab').forEach(t=>t.classList.remove('active'));
  document.querySelectorAll('.pg-panel').forEach(p=>p.classList.remove('active'));
  document.getElementById('pg-tab-'+tab).classList.add('active');
  document.getElementById('pg-panel-'+tab).classList.add('active');
  if(tab==='term')document.getElementById('ex-cmd').focus();
  if(tab==='agent'){document.getElementById('ag-input').focus();if(PG_CUR&&!AG_EVT)agentConnect(PG_CUR);}
}

function termKeydown(e){
  const inp=document.getElementById('ex-cmd');
  if(e.key==='Enter'){runCmd();return}
  if(e.key==='ArrowUp'){
    e.preventDefault();
    if(CMD_HISTORY.length===0)return;
    if(CMD_HIST_IDX===-1){CMD_SAVED=inp.value;CMD_HIST_IDX=CMD_HISTORY.length-1;}
    else if(CMD_HIST_IDX>0){CMD_HIST_IDX--;}
    inp.value=CMD_HISTORY[CMD_HIST_IDX];
    return;
  }
  if(e.key==='ArrowDown'){
    e.preventDefault();
    if(CMD_HIST_IDX===-1)return;
    if(CMD_HIST_IDX<CMD_HISTORY.length-1){CMD_HIST_IDX++;inp.value=CMD_HISTORY[CMD_HIST_IDX];}
    else{CMD_HIST_IDX=-1;inp.value=CMD_SAVED;}
    return;
  }
}

async function runCmd(){
  const inp=document.getElementById('ex-cmd'),out=document.getElementById('ex-out');
  const cmd=inp.value.trim();if(!cmd||!PG_CUR)return;
  CMD_HISTORY.push(cmd);CMD_HIST_IDX=-1;CMD_SAVED='';
  out.innerHTML+='\n<span class="prompt">'+esc(shortCwd())+' $ </span><span class="si">'+esc(cmd)+'</span>\n';
  inp.value='';
  const{s,d}=await api('POST','/api/v1/hiveboxes/'+PG_CUR+'/exec',{command:cmd});
  if(s===200){
    if(d.cwd){CWD=d.cwd;updatePs1();}
    if(d.stdout)out.innerHTML+='<span class="so">'+esc(d.stdout)+'</span>';
    if(d.stderr)out.innerHTML+='<span class="se">'+esc(d.stderr)+'</span>';
    out.innerHTML+='<span class="si">exit '+d.exit_code+' ('+d.duration_ms+'ms)</span>\n';
    refresh();
  }else{
    out.innerHTML+='<span class="se">Error '+s+': '+esc(typeof d==='string'?d:d.error||JSON.stringify(d))+'</span>\n';
  }
  out.scrollTop=out.scrollHeight;
}

// ── Agent playground ────────
let AG_EVT=null; // EventSource
let AG_SES=null; // session id
let AG_BUSY=false;
let AG_BOX=null; // current sandbox for agent

function agentLog(html){
  const out=document.getElementById('ag-out');
  out.innerHTML+=html;
  out.scrollTop=out.scrollHeight;
}

function agentConnect(boxId){
  if(AG_EVT){AG_EVT.close();AG_EVT=null;}
  AG_SES=null;AG_BUSY=false;AG_BOX=boxId;
  const dot=document.getElementById('ag-dot');
  const stat=document.getElementById('ag-status');
  const sesEl=document.getElementById('ag-session');
  dot.className='dot';stat.textContent='Connecting...';sesEl.textContent='';

  document.getElementById('ag-out').innerHTML='<span class="si">~ Connecting to agent for <span class="prompt">'+esc(boxId)+'</span>...</span>\n';

  // SSE — we use fetch+reader instead of EventSource so we can pass auth header
  const url='/api/v1/hiveboxes/'+boxId+'/opencode/event';
  fetch(url,{headers:{'Authorization':'Bearer '+KEY}}).then(r=>{
    if(!r.ok){agentLog('<span class="ev-err">SSE connection failed: '+r.status+'</span>\n');return;}
    dot.className='dot connected';stat.textContent='Connected';
    const reader=r.body.getReader();
    const dec=new TextDecoder();
    let buf='';
    function pump(){
      reader.read().then(({done,value})=>{
        if(done){dot.className='dot';stat.textContent='Disconnected';return;}
        buf+=dec.decode(value,{stream:true});
        const lines=buf.split('\n');
        buf=lines.pop();
        for(const line of lines){
          if(line.startsWith('data: ')){
            try{
              const ev=JSON.parse(line.slice(6));
              handleAgentEvent(ev);
            }catch(e){}
          }
        }
        pump();
      });
    }
    pump();
    // Create session
    agentCreateSession(boxId);
  }).catch(e=>{
    agentLog('<span class="ev-err">Connection error: '+esc(e.message)+'</span>\n');
  });
}

async function agentCreateSession(boxId){
  const{s,d}=await api('POST','/api/v1/hiveboxes/'+boxId+'/opencode/session',{});
  if(s===200&&d.id){
    AG_SES=d.id;
    document.getElementById('ag-session').textContent=d.slug||d.id.substring(0,12);
    agentLog('<span class="si">~ Session: <span class="prompt">'+esc(d.slug||d.id)+'</span></span>\n<span class="ev-sep"></span>');
  }else{
    agentLog('<span class="ev-err">Failed to create session: '+esc(JSON.stringify(d))+'</span>\n');
  }
}

function handleAgentEvent(ev){
  const t=ev.type;
  const p=ev.properties||{};
  const out=document.getElementById('ag-out');
  const dot=document.getElementById('ag-dot');
  const stat=document.getElementById('ag-status');
  const abortBtn=document.getElementById('ag-abort');

  if(t==='server.connected'){
    return; // already handled
  }
  if(t==='server.heartbeat')return;

  if(t==='session.status'){
    const st=p.status?.type||'';
    if(st==='busy'){AG_BUSY=true;dot.className='dot busy';stat.textContent='Thinking...';abortBtn.classList.add('on');}
    if(st==='idle'){AG_BUSY=false;dot.className='dot connected';stat.textContent='Connected';abortBtn.classList.remove('on');}
    return;
  }

  if(t==='message.part.delta'){
    const delta=p.delta||'';
    if(!delta)return;
    // Append text delta inline
    const field=p.field||'text';
    if(field==='text'){
      agentLog('<span class="ev-text">'+esc(delta)+'</span>');
    }else{
      agentLog('<span style="color:#71717a;font-style:italic">'+esc(delta)+'</span>');
    }
    return;
  }

  if(t==='message.part.updated'){
    const part=p.part||{};
    if(part.type==='step-start'){
      agentLog('\n<span class="ev-sep"></span><span class="ev-type">STEP</span>\n');
    }else if(part.type==='step-finish'){
      const tok=part.tokens||{};
      const cost=part.cost||0;
      agentLog('\n<span class="ev-sep"></span><span class="ev-type">STEP DONE</span> <span class="ev-time">reason='+esc(part.reason||'')+' tokens='+JSON.stringify(tok)+'</span>\n');
    }else if(part.type==='tool'){
      const st=part.state||{};
      const toolName=part.tool||'';
      if(st.status==='pending'){
        agentLog('\n<span class="ev-tool">TOOL: '+esc(toolName)+'</span> <span class="ev-time">pending...</span>\n');
      }else if(st.status==='running'){
        const args=st.input||{};
        agentLog('<span class="ev-tool">TOOL: '+esc(toolName)+'</span> <span class="ev-time">running</span>\n');
        if(Object.keys(args).length)agentLog('<span class="ev-data">'+esc(JSON.stringify(args).substring(0,500))+'</span>\n');
      }else if(st.status==='completed'){
        const out=st.output||'';
        agentLog('<span class="ev-tool">TOOL: '+esc(toolName)+'</span> <span class="ev-time">done</span>\n');
        if(out)agentLog('<span class="ev-data">'+esc(typeof out==='string'?out.substring(0,500):JSON.stringify(out).substring(0,500))+'</span>\n');
      }else if(st.status==='error'){
        agentLog('<span class="ev-tool">TOOL: '+esc(toolName)+'</span> <span class="ev-err">error</span>\n');
        if(st.error)agentLog('<span class="ev-err">'+esc(st.error)+'</span>\n');
      }
    }else if(part.type==='tool-invocation'){
      agentLog('\n<span class="ev-tool">TOOL: '+esc(part.toolName||'')+'</span>\n');
      if(part.args)agentLog('<span class="ev-data">'+esc(typeof part.args==='string'?part.args:JSON.stringify(part.args))+'</span>\n');
    }else if(part.type==='tool-result'){
      agentLog('<span class="ev-data">'+esc(typeof part.result==='string'?part.result:JSON.stringify(part.result).substring(0,500))+'</span>\n');
    }
    return;
  }

  if(t==='session.idle'){
    agentLog('\n<span class="ev-sep"></span>');
    return;
  }

  if(t==='session.updated'){
    const info=p.info||{};
    if(info.title){document.getElementById('ag-session').textContent=info.title.substring(0,30);}
    return;
  }

  // Show raw event for anything else
  agentLog('<span class="ev-type">'+esc(t)+'</span> <span class="ev-data">'+esc(JSON.stringify(p).substring(0,300))+'</span>\n');
}

async function agentSend(){
  const inp=document.getElementById('ag-input');
  const text=inp.value.trim();
  if(!text||!AG_SES||!PG_CUR)return;
  inp.value='';
  agentLog('\n<span class="prompt">&gt; '+esc(text)+'</span>\n');
  const{s,d}=await api('POST','/api/v1/hiveboxes/'+PG_CUR+'/opencode/session/'+AG_SES+'/prompt_async',{parts:[{type:'text',text:text}]});
  if(s!==204&&s!==200){
    agentLog('<span class="ev-err">Send failed: '+esc(JSON.stringify(d))+'</span>\n');
  }
}

async function agentAbort(){
  if(!AG_SES||!PG_CUR)return;
  await api('POST','/api/v1/hiveboxes/'+PG_CUR+'/opencode/session/'+AG_SES+'/abort');
}

// Auto-refresh every 5 seconds
setInterval(()=>{if(KEY)refresh()},5000);

// Modal close handlers
document.getElementById('c-modal').addEventListener('click',function(e){if(e.target===this)closeCreate()});
document.addEventListener('keydown',function(e){if(e.key==='Escape'){closeCreate();closePlayground()}});


</script>
</body>
</html>"##;
