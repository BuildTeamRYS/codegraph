import { Cart } from "./cart";

/** Render the cart summary page. */
export function App(): JSX.Element {
  const cart = new Cart();
  cart.add({ id: "p1", name: "Widget", price: 500 });
  return <div>{cart.total([])}</div>;
}
