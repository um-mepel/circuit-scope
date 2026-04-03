// Tests code sinking / ternary simplification patterns.
module sinking_test(input sel, input a, input b, input c, output y1, output y2);
  // sel ? (a + c) : (b + c) → (sel ? a : b) + c  (factor common operand)
  assign y1 = sel ? (a + c) : (b + c);
  // basic ternary
  assign y2 = sel ? a : b;
endmodule
