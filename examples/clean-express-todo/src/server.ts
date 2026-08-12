import express from "express";


import { env } from "./config/env.js";

import { DrizzleTodoRepository } from "./database/drizzle-todo-repository.js";
import { db } from "./database/client.js";
import { TodoController } from "./modules/todos/controller.js";
import { createTodoRouter, errorHandler } from "./modules/todos/routes-express.js";
import { TodoService } from "./modules/todos/service.js";


import { logger } from "./logger.js";


export const app = express();
app.disable("x-powered-by");
app.use(express.json({ limit: "100kb" }));

app.get("/health", (_request, response) => {
  response.status(200).json({ status: "ok" });
});


const todoRepository = new DrizzleTodoRepository(db);
const todoController = new TodoController(new TodoService(todoRepository));
app.use(createTodoRouter(todoController));
app.use(errorHandler);


let server: ReturnType<typeof app.listen> | undefined;

if (env.NODE_ENV !== "test") {
  server = app.listen(env.PORT, () => {
    logger.info({ port: env.PORT }, "Server listening");
  });
}

function shutdown(signal: string): void {
  logger.info({ signal }, "Shutting down");
  server?.close((error) => {
    if (error) {
      logger.error({ error }, "Shutdown failed");
      process.exitCode = 1;
    }
  });
}

process.once("SIGINT", () => shutdown("SIGINT"));
process.once("SIGTERM", () => shutdown("SIGTERM"));