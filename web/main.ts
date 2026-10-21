/**
 * Vite entry point. It lives beside `index.html` because that is where Vite resolves it, and
 * delegates to the app bootstrap so the mount logic sits with the components it mounts.
 */
import "./src/styles/tokens.css";
import { mountApp } from "./src/main";

mountApp();
