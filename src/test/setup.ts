import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach } from "vitest";
import { cleanup } from "@testing-library/react";

class TestStorage implements Storage {
  #items = new Map<string, string>();

  get length() {
    return this.#items.size;
  }

  clear() {
    this.#items.clear();
  }

  getItem(key: string) {
    return this.#items.get(String(key)) ?? null;
  }

  key(index: number) {
    return Array.from(this.#items.keys())[index] ?? null;
  }

  removeItem(key: string) {
    this.#items.delete(String(key));
  }

  setItem(key: string, value: string) {
    this.#items.set(String(key), String(value));
  }
}

const jsdomLocalStorage = new TestStorage();

Object.defineProperty(globalThis, "Storage", {
  configurable: true,
  value: TestStorage,
});

function restoreJsdomLocalStorage() {
  if (globalThis.localStorage === jsdomLocalStorage) return;
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: jsdomLocalStorage,
  });
}

beforeEach(() => {
  restoreJsdomLocalStorage();
});

afterEach(() => {
  cleanup();
  restoreJsdomLocalStorage();
});
