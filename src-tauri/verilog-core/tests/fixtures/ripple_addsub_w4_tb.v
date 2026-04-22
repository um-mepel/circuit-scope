`timescale 1ns/1ns
// Self-contained ripple AddSub (W=4) for CI: 5 + 3 = 8. Exercises `c[i+1]` indices as
// `IrExpr::Binary` after generate unrolling — must merge correctly in flatten *and* module inlining.

module FullAdder(a, b, cin, s, cout);
  input a, b, cin;
  output s, cout;
  assign s = a ^ b ^ cin;
  assign cout = a & b | cin & (a ^ b);
endmodule

module AddSub #(parameter W = 4) (
    input [W-1:0] A,
    input [W-1:0] B,
    input c0,
    output [W-1:0] S,
    output ovf
);
  wire [W:0] c;
  assign c[0] = c0;
  genvar i;
  generate
    for (i = 0; i < W; i = i + 1) begin : RC
      FullAdder FA(A[i], B[i] ^ c[0], c[i], S[i], c[i+1]);
    end
  endgenerate
  assign ovf = c[W-1] ^ c[W];
endmodule

module ripple_addsub_w4_tb();
  wire [3:0] A;
  wire [3:0] B;
  wire c0;
  wire [3:0] S;
  wire ovf;
  assign A = 4'd5;
  assign B = 4'd3;
  assign c0 = 1'b0;
  AddSub dut(A, B, c0, S, ovf);
endmodule
