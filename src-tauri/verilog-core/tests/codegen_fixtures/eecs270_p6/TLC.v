// File Name: TLC.v — EECS 270 Project 6 fixture
module TLC(
  input Clock,
  input Reset,
  input E, NL, EL, W,
  output [6:0] TimerMSD, TimerLSD,
  output reg [6:0] ETL, NLTL, ELTL, WTL
  );

localparam flowA = 2'b00;
localparam flowB = 2'b01;
localparam flowC = 2'b10;

localparam ten = 4'b1010;
localparam one = 4'b0001;
localparam zero = 4'b0000;

localparam green = 7'b0010000;
localparam yellow = 7'b0010001;
localparam red = 7'b0101111;

reg [1:0] flow, next_flow;
reg [3:0] timer, timer_next;
wire none;
assign none = ~E & ~W & ~EL & ~NL;

initial begin
  flow = flowA;
  timer = ten;
end

	always @* begin
    case(flow)
    flowA: if(NL) next_flow <= flowB; else if(EL | none) next_flow <= flowC; else next_flow <= flowA;
    flowB: if(EL) next_flow <= flowC; else if(W | none) next_flow <= flowA; else next_flow <= flowB;
    flowC: if((E & ~NL) | W) next_flow <= flowA; else if(NL | none) next_flow <= flowB; else next_flow <= flowC;
    endcase

    case(timer)
    one: timer_next <= zero;
    zero: timer_next <= ten;
    default: timer_next <= timer - 1;
    endcase
	end

	always @(posedge Clock) begin
    if (Reset) begin
      flow <= flowA;
      timer <= ten;
    end else begin
    timer <= timer_next;
    if (timer_next == ten) flow <= next_flow;
    end

  end

B4to7SEG conv(.Blank(timer == zero), .N(timer), .MSD(TimerMSD), .LSD(TimerLSD));

wire [6:0] color, east_color;
assign color = ((timer == zero) & (next_flow != flow)) ? yellow : green;
assign east_color = ~next_flow[1] ? green : color;

  always @* begin
    case(flow)
    flowA: begin ETL <= east_color; WTL <= color; ELTL <= red; NLTL <= red; end
    flowB: begin ETL <= east_color; WTL <= red; ELTL <= red; NLTL <= color; end
    flowC: begin ETL <= red; WTL <= red; ELTL <= color; NLTL <= red; end
    endcase
	end
endmodule
