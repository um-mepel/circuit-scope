// File Name: B4to7SEG.v
// Converts 4-bit unsigned binary number to two decimal HEX digits
// EECS 270 Project 6 — fixture copy for verilog_core regression tests.
module B4to7SEG(
  input Blank,
  input [3:0] N,
  output [6:0] MSD,
  output [6:0] LSD
);

localparam zero = 7'b1000000;
localparam one = 7'b1111001;
localparam blank = 7'b1111111;

reg[6:0] digits[0:10];

initial begin
  digits[0] = 7'b1111111;
  digits[1] = 7'b1111001;
  digits[2] = 7'b0100100;
  digits[3] = 7'b0110000;
  digits[4] = 7'b0011001;
  digits[5] = 7'b0010010;
  digits[6] = 7'b0000010;
  digits[7] = 7'b1111000;
  digits[8] = 7'b0000000;
  digits[9] = 7'b0011000;
  digits[10] = 7'b1000000;
end

assign LSD = Blank ? blank : digits[N];
assign MSD = Blank ? blank : ((N == 10) ? one : blank);

endmodule
