/**
 * Pad a number to at least `width` digits with leading zeros.
 */
function padNum(n, width) {
  return String(n).padStart(width, '0');
}

module.exports = { padNum };
