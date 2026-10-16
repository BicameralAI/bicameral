// Self-hosted fonts: bundled same-origin so the Cloud CSP (font-src 'self',
// style-src 'self') holds without allowing Google Fonts origins.
import "@fontsource/ibm-plex-sans/latin-400.css";
import "@fontsource/ibm-plex-sans/latin-500.css";
import "@fontsource/ibm-plex-sans/latin-600.css";
import "@fontsource/ibm-plex-sans/latin-700.css";
import "@fontsource/jetbrains-mono/latin-400.css";
import "@fontsource/jetbrains-mono/latin-500.css";
import "@fontsource/jetbrains-mono/latin-600.css";
import "./app.css";
import "./apc1-mobile.css";
import App from "./App.svelte";
import CreateProductUxEval from "./pages/CreateProductUxEval.svelte";
import { hostedRecoveryPath, readHostedHandoff } from "./lib/hosted-handoff";
import { mount } from "svelte";

const evalParam =
  typeof window !== "undefined"
    ? new URLSearchParams(window.location.search).get("eval")
    : null;

// Production App is the only hosted admin runtime. Optional eval harnesses
// (e.g. create-product-ux) stay behind ?eval= for isolated UX experiments.
// Settings is product UI under paired /app — not a separate eval surface.
const Root = evalParam === "create-product-ux" ? CreateProductUxEval : App;

// Cross-visit recovery must run before App mounts. App's legacy unpaired guard
// sends missing handoffs to /connect; a still-live bounded restore credential
// instead goes through Cloud /restore, where Cloud auth and daemon authority
// are both revalidated before a transient session token is re-issued (#1109).
const coldHostedApp =
  typeof window !== "undefined" &&
  evalParam !== "create-product-ux" &&
  window.location.pathname.startsWith("/app") &&
  !readHostedHandoff()?.session_token;

let app: ReturnType<typeof mount> | undefined;
if (coldHostedApp) {
  window.location.replace(hostedRecoveryPath());
} else {
  app = mount(Root, {
    target: document.getElementById("app")!,
  });
}

export default app;
