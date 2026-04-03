// Pure combinational logic — no clock needed.
module comb_logic(input a, input b, output y, output z);
  assign y = a & b;
  assign z = a | b;
endmodule
