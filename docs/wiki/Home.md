# AO3 recommender system

## Summary

This project was done as part of the Data science master of Kschool. The goal of the project is to build a set of recommendation tools that will be able to recommend fanfictions of the [AO3 website](https://archiveofourown.org/). The tools implemented here include a scrapper to download the metadata of the fics that want to be included in the recommender. It also includes tools for recommending fanfics based on a collaborative system based on the likes provided by registered authors and a content based recommender based on the tags associated to each fic. The programs have been tested with a set of 10,000 users and 121,000 fics from the Marvel fandom. While content based recommender did not work as well as expected, collaborative based recommendations worked much better, improving the f1@k and map@k provided by several non-personalized metrics. The set of recommendations and main results can be visualized using the dashboard included in the repository.

## Introduction

The [Archive of our own](https://archiveofourown.org/) is a massive website that contains what is commonly named fanfiction. They are written works done by people all around the work that are based on the premise: what if? They basically take a fantasy universe already created, whether a movie, a book or a manga, and either rewrite part of the history changing some features in it or create a whole new story using the same characters and background information. The archive freely hosts hundreds of thousands of works that go from short stories to book length novels. It functions on a chapter to chapter basis, so many of the works that can be found there may be unfinished or in progress and daily updates are the norm. The works are in first instance grouped by universes which can be accessed from a menu found in the main page of the website. Once inside the main universe page, there is a set of filters that allow the user to search for a work that they may like. These filters are based on tags that the authors of the fanfiction provide upon uploading their work on the website. These tags can be divided in a few categories which include: 

* Fandom, which indicates the name of the universe the work is based on
* Warnings, which are main warnings such as age recommendations or whether there is any kind of adult or sensitive content in the story
* Characters, due to the nature of these kind of writting, fics usually involve the same characters and as such they can be tagged.
* Relationships, which include specific romantic relationships between different characters if present in the story
* Additional tags, these are set by the user and summarize either more explicit warnings of what can be found in the work or a kind of summary of what a user may expect the work to be about.

All these tags can be used to narrow down the search for something to read. Beyond this information the archive also allows users to give likes to the works they have enjoyed and to save things they have read as bookmarks. When a user is registered the likes are saved with the user name and this information can be found for each of the works. As such we have a link between users and things they have enjoyed which will be one of the basis on which these recommender system will be based.

Despite the size of the webpage and the large number of users it has, it does depend on the use of filters to search for things to read. The addition of a recommender system could improve the user experience by allowing them to search more easily for things they may like. With this aim I started working on a set of recommenders that would use explore the performance of different approaches to provide those recommendations.

This work is formed a by a collection of scripts, each of which performs a different flavour of recommendation. In the figure below I detail the four kinds of recommendations I have implemented. The first one includes a set of basic recommendations that are not personalized and can be useful for users that have just started using the archive. Then there are two recommenders based on similarity, one takes tags from the works and users this information to try and added which kind of works a user may be interested in and then recommends such items. The other similarity recommender searches for users that have enjoyed a similar set of items and recommends works that have been read by those users. Finally, a matrix factorization model provides the most accurate predictions found here. Based on the relationships between all users and all items it extracts a set of factors that represent general patterns found in the dataset. Thanks to this abstraction it is able to make recommendations to users not only based on things that other users may have liked but also on items that few people may have read just because the factors are similar. 

<p align="center">
<img src="https://github.com/mmarcet/AO3_recommender_system/blob/master/figures/schemmatic.png" width="500" align="center">
</p>

All the recommenders included here have been tested with a set of users extracted from the archive. The universe of choice was Marvel as it is one of the most prolific universes. At the moment of writting this it contains 439,635 works. For 183,270 of those works I downloaded the metadata and the likes given by registered users. A total of 531,246 registered users had read at least one of the works included in this dataset. This dataset was finally not used in it entirety due to computing capacity, so the analyses detailed here are based on a set of 10,000 users and roughly 121,000 works. Still, the full dataset can be downloaded from [here](https://drive.google.com/file/d/1cd0Aa1G-0J5I5id2sBZ_pCrUagAPK8di/view?usp=sharing)

With the use of the different recommendation scripts we have seen a steady improve in the recommendations going from a very low f1@k and map@k in the random recommendation to much better values in our most complex model:

<p align="center">
<img src="https://github.com/mmarcet/AO3_recommender_system/blob/master/figures/summary_graph.png" width="500" align="center">
</p>

**Acknowledgments**: I would like to thank the managers of the archive of our own for allowing the download of the data used in this work. Also thanks to the many contributors to [towards datascience](https://towardsdatascience.com/data-science/home) and [medium](https://medium.com/topic/data-science) for sharing their experiences. And lastly to Alberto for his advice during the development of this project.