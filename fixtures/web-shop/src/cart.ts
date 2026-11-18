import { Product } from "./product";

/** A shopping cart with quantity tracking. */
export class Cart {
  private items: Map<string, number> = new Map();

  /** Add one unit of `p` to the cart. */
  add(p: Product): void {
    this.items.set(p.id, (this.items.get(p.id) ?? 0) + 1);
  }

  /** Total price across everything in the cart. */
  total(products: Product[]): number {
    let sum = 0;
    for (const p of products) {
      sum += p.price * (this.items.get(p.id) ?? 0);
    }
    return sum;
  }
}

export function cartCapacity(): number {
  return 100;
}
