# frozen_string_literal: true

require 'test_helper'

module ProbabilisticMachines
  class WholeNumberTest < Minitest::Test
    def test_a_fractional_count_is_refused_rather_than_truncated
      sketch = CountMinSketch.new(width: 1024, depth: 4)
      hitters = HeavyHitters.new(k: 16)

      assert_raises(TypeError) { sketch.add('x', count: 0.9) }
      assert_raises(TypeError) { hitters.add('x', count: 0.9) }

      assert_equal 0, sketch.total
      assert_empty hitters.ranked
    end
  end
end
