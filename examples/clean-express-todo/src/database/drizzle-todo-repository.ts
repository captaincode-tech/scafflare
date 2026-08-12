import { desc, eq } from "drizzle-orm";

import type { db } from "./client.js";
import { todos } from "./schema.js";
import type { CreateTodoInput, Todo, UpdateTodoInput } from "../modules/todos/domain.js";
import type { TodoRepository } from "../modules/todos/repository.js";

type Database = typeof db;

export class DrizzleTodoRepository implements TodoRepository {
  public constructor(private readonly database: Database) {}

  public async create(input: CreateTodoInput): Promise<Todo> {
    const now = new Date();
    const [todo] = await this.database.insert(todos).values({
      title: input.title,
      completed: false,
      createdAt: now,
      updatedAt: now,
    }).returning();
    if (todo === undefined) {
      throw new Error("Database did not return the created todo");
    }
    return todo;
  }

  public async list(): Promise<Todo[]> {
    return this.database.select().from(todos).orderBy(desc(todos.id));
  }

  public async findById(id: number): Promise<Todo | null> {
    const [todo] = await this.database.select().from(todos).where(eq(todos.id, id)).limit(1);
    return todo ?? null;
  }

  public async update(id: number, input: UpdateTodoInput): Promise<Todo | null> {
    const [todo] = await this.database.update(todos).set({ ...input, updatedAt: new Date() })
      .where(eq(todos.id, id)).returning();
    return todo ?? null;
  }

  public async delete(id: number): Promise<boolean> {
    const deleted = await this.database.delete(todos).where(eq(todos.id, id)).returning({ id: todos.id });
    return deleted.length === 1;
  }
}