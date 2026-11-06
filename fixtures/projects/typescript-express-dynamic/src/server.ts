import express from "express";
import { health } from "./services";

const app = express();

app.get("/health", health);

export function dispatchDynamic(service: Record<string, () => unknown>, action: string): unknown {
  return service[action]();
}

