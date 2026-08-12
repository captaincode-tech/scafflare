type Environment = {
  NODE_ENV: "development" | "test" | "production";
  PORT: number;
  LOG_LEVEL: "debug" | "info" | "warn" | "error";
  DATABASE_URL?: string | undefined;
};

function parsePort(value: string | undefined): number {
  const port = Number(value ?? "3000");
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error("PORT must be an integer between 1 and 65535");
  }
  return port;
}

function parseNodeEnv(value: string | undefined): Environment["NODE_ENV"] {
  const environment = value ?? "development";
  if (environment !== "development" && environment !== "test" && environment !== "production") {
    throw new Error("NODE_ENV must be development, test, or production");
  }
  return environment;
}

export const env: Environment = {
  NODE_ENV: parseNodeEnv(process.env.NODE_ENV),
  PORT: parsePort(process.env.PORT),
  LOG_LEVEL: (process.env.LOG_LEVEL as Environment["LOG_LEVEL"] | undefined) ?? "info",
  DATABASE_URL: process.env.DATABASE_URL,
};