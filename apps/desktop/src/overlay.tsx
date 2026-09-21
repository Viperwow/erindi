import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { listen } from "@tauri-apps/api/event";
import "./style.css";

function Overlay() {
  const [state, setState] = useState("Idle");
  useEffect(() => {
    const off = listen<string>("app-state", (e) => setState(e.payload));
    return () => void off.then((f) => f());
  }, []);
  return <div class="fixed inset-x-0 bottom-0 h-12 text-center text-white">{state}</div>;
}

render(<Overlay />, document.getElementById("app")!);
