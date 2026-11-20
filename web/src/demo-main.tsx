import { createRoot } from "react-dom/client";
import { PublicDemo } from "./PublicDemo";
import "./styles.css";
import "./workspace.css";
import "./public-demo.css";

createRoot(document.getElementById("root")!).render(<PublicDemo />);
