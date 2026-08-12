import { describe, expect, it } from "vitest";

import type { CreateTodoInput, Todo, UpdateTodoInput } from "../../src/modules/todos/domain.js";
import type { TodoRepository } from "../../src/modules/todos/repository.js";
import { TodoService } from "../../src/modules/todos/service.js";

class MemoryTodoRepository implements TodoRepository {
  private items: Todo[] = [];
  private nextId = 1;

  public async create(input: CreateTodoInput): Promise<Todo> {
    const now = new Date();
    const todo: Todo = { id: this.nextId++, title: input.title, completed: false, createdAt: now, updatedAt: now };
    this.items.push(todo);
    return todo;
  }

  public async list(): Promise<Todo[]> { return [...this.items]; }
  public async findById(id: number): Promise<Todo | null> { return this.items.find((item) => item.id === id) ?? null; }

  public async update(id: number, input: UpdateTodoInput): Promise<Todo | null> {
    const todo = await this.findById(id);
    if (todo === null) return null;
    Object.assign(todo, input, { updatedAt: new Date() });
    return todo;
  }

  public async delete(id: number): Promise<boolean> {
    const before = this.items.length;
    this.items = this.items.filter((item) => item.id !== id);
    return this.items.length !== before;
  }
}

describe("TodoService", () => {
  it("creates, reads, updates and deletes a Todo", async () => {
    const service = new TodoService(new MemoryTodoRepository());
    const created = await service.create({ title: "Ship Scafflare" });
    expect(await service.findById(created.id)).toMatchObject({ title: "Ship Scafflare", completed: false });
    expect(await service.update(created.id, { completed: true })).toMatchObject({ completed: true });
    await service.delete(created.id);
    await expect(service.findById(created.id)).rejects.toMatchObject({ code: "NOT_FOUND" });
  });
});