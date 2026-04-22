`timescale 1ns/1ns
// Mirrors TestBench7 pattern: load a signed value into a slice of SW (EECS 270 Project 7).
module signed_literal_sw_slice_tb();
  reg [17:0] SW;
  initial begin
    SW = 18'd0;
    #10;
    SW[17] = 1'b1;
    SW[10:0] = -11'sd3;
    #10;
  end
endmodule
