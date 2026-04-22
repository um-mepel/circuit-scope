`timescale 1ns/1ns
// Drive AddSub with constant wires (no regs) to verify ripple adder elaboration.
module addsub_const_inputs_tb();
  wire [10:0] A;
  wire [10:0] B;
  wire c0;
  wire [10:0] S;
  wire ovf;
  assign A = 11'd5;
  assign B = 11'd3;
  assign c0 = 1'b0;
  AddSub #(.W(11)) dut(A, B, c0, S, ovf);
endmodule
