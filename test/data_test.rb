# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  # A Data value is held to the core's constructor whether the gem returned it
  # or a caller built it by hand.
  class DataTest < Minitest::Test
    def test_a_hitter_needs_an_overestimate_below_its_count
      assert_equal %w[k 3 2], Hitter.new('k', 3, 2).to_h.values.map(&:to_s)
      assert_equal Hitter.new('k', 3, 2), Hitter.new(key: 'k', count: 3, overestimate: 2)

      assert_raises(RangeError) { Hitter.new('k', 3, 3) }
      assert_raises(RangeError) { Hitter.new('k', 0, 0) }
      assert_raises(RangeError) { Hitter.new('k', -1, 0) }
      assert_raises(RangeError) { Hitter.new('k', 2**64, 0) }
      assert_raises(TypeError) { Hitter.new(:k, 3, 2) }
      assert_raises(TypeError) { Hitter.new('k', 3.0, 2) }
    end

    def test_a_centroid_needs_a_finite_mean_and_a_finite_positive_weight
      centroid = Centroid.new(1, 2)

      assert_equal [1.0, 2.0], [centroid.mean, centroid.weight]
      assert_instance_of Float, centroid.mean

      [[Float::NAN, 1], [Float::INFINITY, 1], [1, 0], [1, -1], [1, Float::INFINITY]].each do |mean, weight|
        assert_raises(RangeError) { Centroid.new(mean, weight) }
      end
      assert_raises(TypeError) { Centroid.new('1', 1) }
      assert_raises(TypeError) { Centroid.new(1, nil) }
    end

    # A subclass overriding `initialize` without `super`, `allocate`, or a
    # call to a hook itself would skip the core's check.
    def test_values_are_final_and_their_hooks_private
      [Hitter, Centroid].each do |value|
        assert_raises(TypeError) { Class.new(value) { def initialize(**) = nil } }
        assert_raises(NoMethodError) { value.allocate }
      end
      assert_raises(RangeError) { Centroid.new(1, 2).with(weight: 0) }
      assert_raises(NoMethodError) { ProbabilisticMachines.__hitter('k', 1, 0) }
      assert_raises(NoMethodError) { ProbabilisticMachines.__centroid(1.0, 1.0) }
    end
  end
end
