// Tests peephole / instruction combining patterns.
module peephole_test(input a, input b, output y1, output y2, output y3);
  // a + a → a << 1
  assign y1 = a + a;
  // a - a → 0 (already handled by algebraic, but tests chain)
  assign y2 = a - a;
  // basic expression
  assign y3 = a + b;
endmodule
