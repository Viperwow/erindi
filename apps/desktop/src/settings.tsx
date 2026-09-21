import { render } from "preact";
import "./style.css";

function Settings() {
  return <main class="p-6 font-sans text-sm">Whispio settings</main>;
}

render(<Settings />, document.getElementById("app")!);
