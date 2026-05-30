from manim import *
from typing import cast
import numpy as np
import math




class GDL(MovingCameraScene):
    def construct(self):
        axis_config = {
            "tick_size": 0
        }
        camera = cast(MovingCamera, self.camera)
    
        axes1 = Axes(
            x_range=[0, 1, 1],
            y_range=[0, 1, 1],
            x_length=2,
            y_length=2,
            axis_config=axis_config,
        ).shift(2 * LEFT + 2 * UP)

        axis2 = Axes(
            x_range=[0, 1, 1],
            y_range=[0, 1, 1],
            x_length=2,
            y_length=2,
            axis_config=axis_config,
        ).shift(1.5 * RIGHT + 1.2 * DOWN)
        n1_1 = Dot(axes1.c2p(0, 0), color=RED, radius=0.15)
        n2_1 = Dot(axis2.c2p(0, 0), color=BLUE, radius=0.15)
        d1 = DashedLine(n1_1.get_center(), n2_1.get_center())
    
        d1_l = MathTex("d_1").next_to(d1, DOWN, buff=0.005).rotate(d1.get_angle()).shift(LEFT*0.5, UP*1)
        self.play(FadeIn(axes1), FadeIn(axis2))
        self.wait(2)
        self.add(n1_1, n2_1)
        self.wait(1)
        self.play(Create(d1), Create(d1_l))
        self.wait(1.5)

        n1_2 = Dot((-0.7, 1.8, 0), color=RED, radius=0.15)
        v1 = Line(n1_1.get_center(), n1_2.get_center())
        v1_l = MathTex("v_1").next_to(v1,  direction=UP, buff=0.05).rotate(v1.get_angle()).shift(LEFT*0.2, DOWN * 0.2)
        alpha1 = Angle(axes1.x_axis, v1, radius=1)
        alpha1_l = MathTex("\\alpha_1").next_to(alpha1).shift(0.1 * UP)

        n2_2 = Dot((1.4, 1.6, 0), color=BLUE, radius=0.15)
        v2 = Line(n2_1.get_center(), n2_2.get_center())
        v2_l = MathTex("v_2").next_to(v2,  direction=UP, buff=0.05).rotate(v1.get_angle()).shift(LEFT*0.3, DOWN * 0.7)
        alpha2 = Angle(axis2.x_axis, v2, radius=1)
        alpha2_l = MathTex("\\alpha_2").next_to(alpha2).shift(0.1 * UP)
        
        d2 = DashedLine(n1_2.get_center(), n2_2.get_center())
        d2_l = MathTex("d_2").next_to(d2, direction=UP, buff=0.05).rotate(d2.get_angle())
        self.play(Create(v1), Create(v1_l), Create(v2), Create(v2_l))
        self.add(alpha1, alpha1_l, alpha2, alpha2_l)
        self.add(n1_2, n2_2)
        self.wait(1)
        self.play(Create(d2), Create(d2_l))
        self.wait(10) ## STOP 1

        # n1's reference frame
        self.remove(axis2, alpha2, alpha2_l, v2, n2_1, n2_2)
        circle1 = DashedVMobject(Circle(d1.get_length(), color=PURE_CYAN).shift(n1_1.get_center()), num_dashes=20)
        circle2_full = Circle(d2.get_length()).shift(n1_2.get_center())
        circle2_arc_start = Arc(d2.get_length(), d2.get_angle(), -130 * DEGREES - d2.get_angle()).shift(n1_2.get_center())
        circle2_arc = Arc(d2.get_length(), -130 * DEGREES, 220 * DEGREES).shift(n1_2.get_center())
        circle2 = DashedVMobject(circle2_full, num_dashes=40)

        self.play(camera.frame.animate.move_to(n1_1.get_center()).set(width=circle1.width * 1.2, height=circle1.height * 1.2))
        self.play(Create(circle1))
        self.play(Create(circle2))
        self.wait(10) ## STOP 2
        n2_loose = Dot(d2.get_end(), radius=0.15, color=BLUE)
        self.add(n2_loose)
        self.wait(0.5)

        d2.add_updater(
            lambda d: d.put_start_and_end_on(
                n1_2.get_center(),
                n2_loose.get_center()
            )
        )

        self.play(MoveAlongPath(n2_loose, circle2_arc_start),
                  run_time=1, rate_func=smooth)
        self.play(MoveAlongPath(n2_loose, circle2_arc),
                  run_time=4, rate_func=there_and_back)
        self.play(MoveAlongPath(n2_loose, circle2_arc_start.reverse_points()),
                  run_time=1, rate_func=smooth)
        self.wait(5) ## STOP 3
        v2.reverse_direction()
        offset = v2.get_end() - v2.get_start()
        v2.add_updater(
            lambda d: d.put_start_and_end_on(
                n2_loose.get_center(),
                v2.get_start() + offset
            )
        )
        self.play(Create(v2))
        trace = TracedPath(v2.get_end)
        self.add(trace)
        self.play(MoveAlongPath(n2_loose, circle2_full.rotate(d2.get_angle())),
                  run_time=5, rate_func=smooth)
        d2.clear_updaters()
        v2.clear_updaters()
        self.wait(5) ## STOP 4

    
        intersections = self.calculate_intersections(v1.get_length(), v2.get_length(), d1.get_length(), d2.get_length(), alpha1.get_value(), alpha2.get_value())

        n2_candidates = []
        for p in intersections:
            c = Dot(n1_1.get_center() + (p[0], p[1], 0.0), color="GREEN", radius=0.15)
            n2_candidates.append(c)
            self.add(c)

        self.wait(5)
        self.remove(trace, circle1, circle2, v2, v2_l, v1, v1_l, d1, d1_l, d2, d2_l, alpha1, alpha1_l, n1_2, n2_loose)
        self.wait(15) ## STOP 5
        
        
        #### Verification with third point
        n3_1 = Dot((0,0,0), color="PINK", radius=0.15).shift(LEFT * 6, UP * 2)
        n3_2 = Dot(n3_1.get_center(), color="PINK", radius=0.15).shift(LEFT, DOWN * 2)
        self.add(n3_1, n3_2)
        self.wait(10) ## STOP 5.5
        d23 = DashedLine(n3_2, n2_candidates[0], color="YELLOW")
        all_d23 = [
            d23,
            DashedLine(n3_2, n2_candidates[1], color="YELLOW"),
            DashedLine(n3_1, n2_candidates[0], color="YELLOW"),
            DashedLine(n3_1, n2_candidates[1], color="YELLOW"),
        ]

        d23_example = Line(Point(LEFT * 7 + UP * 5), Point(LEFT * 7 + UP * 5 + d23.get_length() * RIGHT), color="RED")
        d23_ex_l = MathTex("d_{23}").next_to(d23_example)
        d23_tomove =  Line(Point(LEFT * 7 + UP * 5), Point(LEFT * 7 + UP * 5 + d23.get_length() * RIGHT), color="RED")
        

        self.play(Create(d23_example), Create(d23_ex_l), Create(d23_tomove))
        self.wait(1)
        self.play(Create(all_d23[0]))
        self.play(Create(all_d23[1]))
        self.play(Create(all_d23[2]))
        self.play(Create(all_d23[3]))
        self.wait(2)

        target = Line(color="RED", stroke_width=10)
        target.put_start_and_end_on(d23.get_start(), d23.get_end())
        # self.remove(d23)
        d23_tomove.generate_target()
        d23_tomove.target = target
        self.play(MoveToTarget(d23_tomove))
        self.wait(10) ## STOP 6

        [self.remove(l) for l in all_d23]
        self.remove(n3_1, d23_example, d23_ex_l, d23_tomove, n3_2, n2_candidates[0], n2_candidates[1])
        self.add(n1_1)
        self.play(Create(v2.reverse_points()), Create(n2_1), Create(n2_2), Create(v1), Create(n1_2))

        self.wait(10)

        n1_coord = MathTex("(x_0, y_0)").next_to(n1_1, direction=LEFT)
        n1_2_coord = MathTex("(x_1, y_1)").next_to(n1_2, direction=UP)
        n2_coord = MathTex("(x_2, y_2)").next_to(n2_1, direction=DOWN)
        n2_2_coord = MathTex("(x_3, y_3)").next_to(n2_2, direction=UP)

        self.play(Create(d1), Create(d2),
                  Create(axis2),
                  Create(alpha1), Create(alpha1_l),
                  Create(alpha2), Create(alpha2_l),
                  Create(n1_coord), Create(n1_2_coord),
                  Create(n2_coord), Create(n2_2_coord)
                  )
        self.wait(2)

        self.play(camera.frame.animate.move_to(RIGHT * 5.3))
        eq_world = VGroup()
        eq_x1 = MathTex("x_1 = v_1 * cos(\\alpha)")
        eq_y1 = MathTex("y_1 = v_1 * sin(\\alpha)").shift(DOWN * 0.5)
        eq_x3 = MathTex("x_3 = x_2 + v_2 * cos(\\alpha)").shift(DOWN)
        eq_y3 = MathTex("y_3 = y_2 + v_2 * sin(\\alpha) ").shift(DOWN * 1.5)

        eq_d2 = MathTex("(x_3 - x_1)^2 + (y_3 - y_1)^2 = d_2^2").shift(DOWN * 2)
        eq_d1 = MathTex("x_2^2 + y_2^2 = d_1^2").shift(DOWN * 2.5)
        eq_world.add(eq_x1, eq_y1, eq_x3, eq_y3, eq_d2, eq_d1)
        eq_world.shift(RIGHT * 8 + UP * 4)
        self.play(Create(eq_world))
        self.wait(15) ## STOP 7

        variables = VGroup()
        solution = VGroup()
        A = MathTex("A = v_2 * cos(\\alpha_2) - v_1 * cos(\\alpha_1)")
        B = MathTex("B = v_2 * sin(\\alpha_2) - v_1 * sin(\\alpha_1)").shift(DOWN*0.5)
        C = MathTex("C = 0.5 * (d_2^2 - d_1^2 - v_1^2 - v_2^2+ 2 * v_1 * v_2 * cos(\\alpha_1 - \\alpha_2)").shift(DOWN + RIGHT)
        eq_x2 = MathTex("x_2 = \\frac{AC +- \\sqrt{A^2C^2 - (A^2 + B^2) * (C^2-d_1^2B^2)}}{A^2 + B^2}").shift(DOWN*2.5 + RIGHT)
        eq_y2 = MathTex("y_2 = \\frac{BC +- \\sqrt{B^2C^2 - (A^2 + B^2) * (C^2-d_1^2A^2)}}{A^2 + B^2}").shift(DOWN*4 + RIGHT)

        variables.add(A, B, C)
        variables.shift(RIGHT * 8 + UP)
        self.play(Create(variables))
        self.wait(4)
        solution.add(eq_x2, eq_y2)
        solution.shift(RIGHT * 8 + UP)
        self.play(Create(solution))

        self.wait(20)


        ## Exceptional configuration

        self.clear()
        camera.frame.shift(LEFT * 5.3).set(width = camera.frame.width * 1.3)
        self.add(axes1, axis2, n1_1, n2_1, d1)
        n1_2 = Dot(n1_1.get_center() + RIGHT * 2 + UP, color=RED, radius=0.15)
        n2_2 = Dot(n2_1.get_center() + RIGHT * 2 + UP, color=BLUE, radius=0.15)
        v1 = Line(n1_1.get_center(), n1_2.get_center())
        v2 = Line(n2_1.get_center(), n2_2.get_center())
        d2 = DashedLine(n1_2.get_center(), n2_2.get_center())

        self.play(Create(v1), Create(v2), Create(n1_2), Create(n2_2))
        self.play(Create(d2))
        self.wait(2)

        circle1 = DashedVMobject(Circle(d1.get_length(), color=PURE_CYAN).shift(n1_1.get_center()), num_dashes=20)
        circle2_full = Circle(d2.get_length()).shift(n1_2.get_center())
        circle2 = DashedVMobject(circle2_full, num_dashes=40)
        self.play(Create(circle1))
        self.play(Create(circle2))
        self.wait(2)

        d2.add_updater(
            lambda d: d.put_start_and_end_on(
                n1_2.get_center(),
                n2_2.get_center()
            )
        )
        v2.reverse_direction()
        offset = v2.get_end() - v2.get_start()
        v2.add_updater(
            lambda d: d.put_start_and_end_on(
                n2_2.get_center(),
                v2.get_start() + offset
            )
        )
        # self.play(Create(v2))
        self.play(MoveAlongPath(n2_2, circle2_full),
                  run_time=10, rate_func=smooth)

        self.wait(10)




    def calculate_intersections(self, v1, v2, d1, d2, alpha1, alpha2) -> tuple[tuple[float, float], tuple[float, float]]:
        A = v2 * math.cos(alpha2) - v1 * math.cos(alpha1)
        B = v2 * math.sin(alpha2) - v1 * math.sin(alpha1)
        C = 0.5 * (
            d2**2 - d1**2 - v1**2 - v2**2
            + 2 * v1 * v2 * math.cos(alpha1 - alpha2)
        )

        D = A**2 + B**2
        E = A * C
        F = C**2 - d1**2 * B**2
        G = B * C
        H = C**2 - d1**2 * A**2

        x_roots = self.solve_special_quadratic(D, E, F)
        y_roots = self.solve_special_quadratic(D, G, H)

        return (
            (x_roots[0], y_roots[0]),
            (x_roots[1], y_roots[1]),
        )


    def solve_special_quadratic(self, D, E, F) -> tuple[float, float]:
        disc = E**2 - D * F

        sqrt_disc = math.sqrt(disc)

        return (
            (E + sqrt_disc) / D,
            (E - sqrt_disc) / D,
        )