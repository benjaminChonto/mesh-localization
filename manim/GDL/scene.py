from manim import *
from typing import cast
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
    
        d1_l = Text("d1", font_size=32).next_to(d1, DOWN, buff=0.005).rotate(d1.get_angle()).shift(LEFT*0.5, UP*1)
        self.play(FadeIn(axes1), FadeIn(axis2))
        self.wait(2)
        self.add(n1_1, n2_1)
        self.wait(1)
        self.play(Create(d1), Create(d1_l))
        self.wait(1.5)

        n1_2 = Dot((-0.7, 1.8, 0), color=RED, radius=0.15)
        v1 = Line(n1_1.get_center(), n1_2.get_center())
        v1_l = Text("v1", font_size=32).next_to(v1,  direction=UP, buff=0.05).rotate(v1.get_angle()).shift(LEFT*0.2, DOWN * 0.2)
        alpha1 = Angle(axes1.x_axis, v1, radius=1)
        alplha1_l = Text("⍺1", font_size=24).next_to(alpha1).shift(0.1 * UP)

        n2_2 = Dot((1.4, 1.6, 0), color=BLUE, radius=0.15)
        v2 = Line(n2_1.get_center(), n2_2.get_center())
        v2_l = Text("v2", font_size=32).next_to(v2,  direction=UP, buff=0.05).rotate(v1.get_angle()).shift(LEFT*0.3, DOWN * 0.7)
        alpha2 = Angle(axis2.x_axis, v2, radius=1)
        alpha2_l = Text("⍺2", font_size=24).next_to(alpha2).shift(0.1 * UP)
        
        d2 = DashedLine(n1_2.get_center(), n2_2.get_center())
        d2_l = Text("d2", font_size=32).next_to(d2, direction=UP, buff=0.05).rotate(d2.get_angle())
        self.play(Create(v1), Create(v1_l), Create(v2), Create(v2_l))
        self.add(alpha1, alplha1_l, alpha2, alpha2_l)
        self.add(n1_2, n2_2)
        self.wait(1)
        self.play(Create(d2), Create(d2_l))
        self.wait(2)

        # n1's reference frame
        self.remove(axis2, alpha2, v2, n2_1, n2_2)
        circle1 = DashedVMobject(Circle(d1.get_length(), color=PURE_CYAN).shift(n1_1.get_center()), num_dashes=20)
        circle2_full = Circle(d2.get_length()).shift(n1_2.get_center())
        circle2_arc_start = Arc(d2.get_length(), d2.get_angle(), -130 * DEGREES - d2.get_angle()).shift(n1_2.get_center())
        circle2_arc = Arc(d2.get_length(), -130 * DEGREES, 220 * DEGREES).shift(n1_2.get_center())
        circle2 = DashedVMobject(circle2_full, num_dashes=40)

        self.play(camera.frame.animate.move_to(n1_1.get_center()).set(width=circle1.width * 1.2, height=circle1.height * 1.2))
        self.play(Create(circle1))
        self.play(Create(circle2))
        self.wait(1)
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
        self.wait(1)
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
        self.wait(3)

