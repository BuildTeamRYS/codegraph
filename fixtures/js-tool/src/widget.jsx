/**
 * Tiny counter button component.
 */
export function Counter({ label }) {
  return <button onClick={() => alert(label)}>{label}</button>;
}
