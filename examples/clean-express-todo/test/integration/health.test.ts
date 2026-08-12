import { describe, expect, it } from "vitest";


import request from "supertest";
import { app } from "../../src/server.js";

describe("GET /health", () => {
  it("returns an OK health response", async () => {
    const response = await request(app).get("/health");
    expect(response.status).toBe(200);
    expect(response.body).toEqual({ status: "ok" });
  });
});
