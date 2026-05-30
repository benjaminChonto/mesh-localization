from typing import cast

from manim import *
import numpy as np
import math


class TwoStepMotionAlgorithm(MovingCameraScene):
    def construct(self):
        camera = cast(MovingCamera, self.camera)
        self.play(camera.frame.animate.set(width=camera.frame_width * 1.6))

        b = Dot(RIGHT + DOWN, color=BLUE, radius=0.15)
        r = Dot(LEFT + UP, color=RED, radius=0.15)
        arr1 = Arrow(r.get_center(), r.get_center() + UP * 2, stroke_width=2, tip_length=0.25)
        arr1_n = arr1.copy()
        r1_l = MathTex("(x_1, x_2)").next_to(r, direction=LEFT)

        self.add(b, r, r1_l)
        self.play(Create(arr1), Create(arr1_n))

        d1_group = VGroup()
        d1 = DashedLine(b.get_center(), r.get_center())
        d1_l = MathTex("d_1").next_to(d1, direction=LEFT)
        d1_group.add(d1, d1_l)
        self.play(Create(d1_group))
        self.wait(1)

        c1 = DashedVMobject(Circle(d1.get_length(), color=LIGHT_GREY).shift(r.get_center()))
        self.play(Create(c1))
        self.wait(1)

        self.play(
            Rotate(
                arr1_n,
                -1.3,
                about_point=r.get_center()
            )
        )
        theta1_group = VGroup()
        theta1 = Angle(arr1_n, arr1)
        theta1_l = MathTex("\\theta_1").next_to(theta1, direction=UP)
        theta1_group.add(theta1, theta1_l)
        self.play(Create(theta1_group))
        self.wait(2)

        ## First step
        direction1 = arr1_n.get_end() - arr1_n.get_start()
        arr1_nm = arr1_n.copy()
        self.add(arr1_nm)
        self.play(
            r.animate.shift(direction1 * 1.5),
            arr1_n.animate.shift(direction1 * 1.5)
        )
        r2_l = MathTex("(x_0, x_0)").next_to(r, direction=UP)
        self.play(Create(r2_l))

        self.wait(1)
        d2_group = VGroup()
        d2 = DashedLine(r.get_center(), b.get_center())
        d2_l = MathTex("d_2").next_to(d2, direction=LEFT)
        d2_group.add(d2, d2_l)
        self.play(Create(d2_group))
        self.wait(1)
        c2 = DashedVMobject(Circle(d2.get_length(), color=LIGHT_PINK).shift(r.get_center()))
        self.play(Create(c2))
        self.wait(1)

        ## Second step
        arr2 = arr1_n.copy()
        self.add(arr2)

        self.play(Rotate(
                arr2,
                -0.8,
                about_point=r.get_center()
        ))

        theta2_group = VGroup()
        theta2 = Angle(arr2, arr1_n)
        theta2_l = MathTex("\\theta_2").next_to(theta2, direction=RIGHT)
        theta2_group.add(theta2, theta2_l)
        self.play(Create(theta2_group))
        self.wait(2)

        self.add(arr2.copy())
        direction2 = arr2.get_end() - arr2.get_start()
        self.play(
            r.animate.shift(direction2 * 2),
            arr2.animate.shift(direction2 * 2)
        )
        r3_l = MathTex("(x_2, x_2)").next_to(r)
        self.play(Create(r3_l))
        self.wait(1)

        d3_group = VGroup()
        d3 = DashedLine(r.get_center(), b.get_center())
        d3_l = MathTex("d_3").next_to(d3, direction=DOWN)
        d3_group.add(d3, d3_l)
        self.play(Create(d3_group))
        self.wait(1)
        c3 = DashedVMobject(Circle(d3.get_length(), color=LIGHT_BROWN).shift(r.get_center()))
        self.play(Create(c3))
        self.wait(1)
        self.add(Dot(b.get_center(), radius=0.2, color=BLUE))

        ## Equations
        self.play(camera.frame.animate.shift(RIGHT * 7.5))

        eqs = VGroup()
        d1_eq = MathTex("(x_3 - x_1)^2 + (y_3 - y_1)^2 = d_1^2")
        d2_eq = MathTex("(x_3 - x_2)^2 + (y_3 - y_2)^2 = d_1^2").shift(DOWN * 0.5)
        d0_eq = MathTex("x_3^2 + y_3^2 = d_0^2").shift(DOWN)
        eqs.add(d1_eq, d2_eq, d0_eq)
        eqs.shift(RIGHT * 11.5 + UP * 4)
        self.play(Create(eqs))
        self.wait(1)

        pos_eq = VGroup()
        x1_poz = MathTex("x_1 = v_1 * cos(\\theta_1 - \\pi)")
        y1_poz = MathTex("y_1 = v_1 * sin(\\theta_1 - \\pi)").shift(DOWN * 0.5)
        x2_poz = MathTex("x_2 = v2 * cos(\\theta_2)").shift(DOWN * 1)
        y2_poz = MathTex("y_2 = v2 * sin(\\theta_2)").shift(DOWN * 1.5)
        pos_eq.add(x1_poz, y1_poz, x2_poz, y2_poz)
        pos_eq.shift(RIGHT * 11.5 + UP * 2)
        self.play(Create(pos_eq))
        self.wait(10)

        solution = VGroup()
        x3_eq = MathTex("x_3 = \\frac{d_0^2 + v_1^2 - 2v_1y_3*sin(\\theta_1 - \\pi) - d_1^2}{2*v_1*cos(\\theta_1-\\pi)}")
        y3_eq = MathTex("y_3 = \\frac{d_2^2 - d_0^2 - v_2*2 + \\frac{v_2cos(\\theta_2)}{v_1cos(\\theta_1-\\pi)}(d_0^2+v_1^2-d_1^2)}{\\frac{2v_1sin(\\theta_1-(\\pi+\\theta_2))}{cos(\\theta_1-\\pi)}}").shift(DOWN*2)
        solution.add(x3_eq, y3_eq)
        solution.shift(RIGHT * 11.5 + DOWN * 2)
        self.play(Create(solution))
        self.wait(15)

        ## Special case
        self.clear()
        camera.frame.shift(LEFT * 7.5)
        camera.frame.set(width = camera.frame_width * 0.8)
        b = Dot(RIGHT + DOWN, color=BLUE, radius=0.15)
        r = Dot(LEFT + UP, color=RED, radius=0.15)
        arr1 = Arrow(r.get_center(), r.get_center() + UP * 2, stroke_width=2, tip_length=0.25)
        arr1_n = arr1.copy()
        r1_l = MathTex("(x_1, x_2)").next_to(r, direction=LEFT)
        self.add(b, r, arr1, arr1_n, d1, c1)

        arr2 = arr1_n.copy()
        self.add(arr2)

        self.play(Rotate(
                arr2,
                -d1.get_angle(),
                about_point=r.get_center()
        ))
        theta1_group = VGroup()
        theta1 = Angle(arr2, arr1_n)
        theta1_l = MathTex("\\theta_1").next_to(theta1, direction=UP)
        theta1_group.add(theta1, theta1_l)
        self.play(Create(theta1_group))
        self.wait(2)

        ## First step
        direction1 = arr2.get_end() - arr2.get_start()
        # arr1_nm = arr1_n.copy()
        # self.add(arr1_nm)
        self.play(
            r.animate.shift(direction1 * 0.8),
            arr2.animate.shift(direction1 * 0.8)
        )
        self.wait(1)
        d2 = DashedLine(r.get_center(), b.get_center())
        self.play(Create(d2))
        self.wait(1)
        c2 = DashedVMobject(Circle(d2.get_length(), color=LIGHT_PINK).shift(r.get_center()))
        self.play(Create(c2))
        self.play(Create(Dot(b.get_center(), color=BLUE, radius=0.15)))

        self.wait(15)
