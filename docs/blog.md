---
layout: default.liquid
permalink: "/posts/"
title: Blog
description: Notes on image forensics and on this library.
---

Longer-form writing. For reference material, see the
[documentation](/docs/installation/) instead.

<ul class="post-list">
{% for post in collections.posts.pages %}
<li>
    <a href="/{{ post.permalink }}">{{ post.title }}</a>
    {% if post.published_date %}
    <time datetime="{{ post.published_date | date: '%Y-%m-%d' }}">
        {{ post.published_date | date: "%B %d, %Y" }}
    </time>
    {% endif %}
    {% if post.description %}<p>{{ post.description }}</p>{% endif %}
</li>
{% endfor %}
</ul>
